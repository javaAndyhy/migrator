# migrator — 多目标 Agent 配置迁移引擎

> 把 Claude Code / Codex 与国产 AI 编程平台（Qoder / Trae / 灵码(现 Qoder CN) / WorkBuddy / DeepSeek Harness(dsh) / 豆包 / MarsCode...）的配置**双向**迁移。
> 平台中立 · 安全优先 · 社区可扩展

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/javaAndyhy/migrator)](https://github.com/javaAndyhy/migrator/releases)

**中文** | [English](README.en.md)

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
# 目标平台: qoder | trae | lingma(现 Qoder CN) | workbuddy | dsh | doubao (默认 qoder)
migrator --source codex --target qoder apply --project . --yes
migrator --source claude --target trae apply --project . --yes
migrator --source codex --target lingma apply --project . --yes
migrator --source codex --target workbuddy apply --project . --yes
migrator --source codex --target dsh apply --project . --yes       # DeepSeek Harness
migrator --source claude --target doubao apply --project . --yes   # 豆包（生成手动粘贴包）

# 反向迁移（国产 → Claude / Codex）
migrator --source qoder --target claude apply --project . --yes
migrator --source workbuddy --target codex apply --project . --yes
migrator --source dsh --target claude apply --project . --yes

# 只迁某个配置面（不碰 MCP，避免客户端重载连接器 / 弹授权）
# 面: instructions | mcp | skills | agents | commands | hooks | memory
migrator --source codex --target workbuddy --only instructions apply --project . --yes
migrator --source codex --target dsh --only mcp,skills apply --project . --yes

# 校验目标
migrator --source codex validate --project .
```

### 目标平台差异

| 平台 | 指令文件 | MCP 配置 | 配置根目录 |
|---|---|---|---|
| Qoder | `AGENTS.md`（项目根） | `.mcp.json`（merge 保留已有） | `.qoder/` |
| Trae | `AGENTS.md`（项目根） | `mcp.json`（merge 保留已有） | `.trae/` |
| 通义灵码（已更名 Qoder CN） | `AGENTS.md`（项目根；IDE 规则另存 `.lingma/rules/`） | `mcp-settings.json`（partial） | `.lingma/` |
| WorkBuddy | `MEMORY.md`（托管块引用） | `mcp.json`（用户级 `~/.workbuddy/`，merge 保留已有） | `.workbuddy/` |
| **DeepSeek Harness（dsh）** | `~/.dsh/AGENTS.md`（用户级全局） | **`~/.dsh/profiles/<profile>/cordis.patch.yml`**（cordis patch YAML 插件条目，非 JSON） | `.dsh/` |
| **豆包（Doubao desktop）** | 手动包 `.doubao/manual/instructions.md` | 手动包 `.doubao/manual/connectors.json`（客户端无文件级契约，需 UI 粘贴） | `.doubao/` |

> **DeepSeek Harness 平台特例**（2026-10-26 本机实测 + `@deepseek-ai/dsh-*` 官方包 README 核实）：
> - instructions：用户级恒为 `$DSH_HOME/AGENTS.md`（默认 `~/.dsh/AGENTS.md`），dsh 先加载它、再按 projectRoot→cwd 逐层加载项目侧 `AGENTS.md`/`CLAUDE.md`（+ `*.local.md` 覆盖层）。迁移取用户级，与 Codex 的 `~/.codex/AGENTS.md` 全局规则语义对齐。
> - MCP：**不是** `mcp.json`，而是 profile 的 cordis patch 插件条目（`name: '@deepseek-ai/dsh-mcp-client'`，`config.serverName/transport/command/args/env/url/headers`）。写入采用 **YAML 注释托管块** `# <!-- BEGIN MIGRATOR:dsh:mcp -->`，块外用户条目、注释与 `!!js` 表达式逐字保留（解析再序列化会摧毁它们）；`{env:VAR}` 占位符转 `!!js process.env.VAR`，反向再还原。
> - skills：`~/.dsh/skills`（user-dsh 根，另有项目级 `.dsh/skills`）→ 迁移写用户级。
> - agents：subagent 由 profile/preset 驱动，**无**文件级 `agents/*.md` → `unsupported`；hooks / memory 同理。

> **豆包平台特例**（2026-10-26 本机实测）：豆包桌面客户端**没有文件级配置契约** —— MCP「自定义连接器」、技能、规则都存在客户端内部（`%LOCALAPPDATA%\Doubao\User Data\Default\Local Storage\leveldb`），只能由用户在 UI 粘贴。因此本工具输出「手动迁移包」`{project}/.doubao/manual/`（`connectors.json` / `instructions.md` / `skills/` / `agents/` / `README.md` 操作说明），并明确标注目标程序不会读取这些文件。

> **平台演进事实**（2026-08-18 经 context7/官网核实）：
> - **Qoder**：规则兼容 AGENTS.md，官方要求放在**项目根**（docs.qoder.com/user-guide/rules）；Custom Agent 为 `.md` + YAML frontmatter（name/description/tools/model/skills/mcpServers）。
> - **通义灵码 → Qoder CN**：阿里已将灵码整合进 Qoder CN 系列（IDE / JetBrains / VS Code 插件）。规则机制改为 `.lingma/rules/` 目录（IDE）+ 三级 AGENTS.md（CLI：用户级 `~/.qoder/AGENTS.md` / 项目级 / 本地级）；`LINGMA.md` 已弃用。agents 在 `.lingma/agents/*.md`。
> - **Trae**：品牌演进为 TraeWork（docs.trae.ai），本工具 Trae 布局基于既有官方信息（AGENTS.md + `.trae/mcp.json`），待复核。

> **WorkBuddy 平台特例**（实测 2026-08-17）：instructions 全文写入 `~/.workbuddy/rules/`，`MEMORY.md` 只保留托管块引用（避免数百行规则超出记忆容量）；MCP 合并到用户级 `~/.workbuddy/mcp.json`（官方路径，保留已有 server）；agents / memory 无对应物 → `unsupported`（WorkBuddy 无文件级 subagent，无只读记忆索引消费方）。

平台间差异集中在"输出布局"，新增平台只需在 `src/target/layout.rs` 定义一个布局 + 写映射表 JSON。

### 三个国产目标的真机实测状态（2026-10-26）

同一份 `~/.codex` 源，`dsh` / `workbuddy` / `doubao` 三个目标全部在本机装好的真实客户端上跑通，
不是纸面实现：

| 目标 | 实测产物 | 断言结果 |
|---|---|---|
| `dsh` | `~/.dsh/AGENTS.md`、`~/.dsh/profiles/desktop/cordis.patch.yml`、`~/.dsh/skills/`×59 | YAML 语法合法；19 条条目（用户原有 6 + MCP 13），id 唯一；重复 apply 文件 md5 不变 |
| `workbuddy` | `~/.workbuddy/mcp.json`、`MEMORY.md`（394 字节引用块）、`rules/codex-rules.md`、`skills/`×59 | `mcp.json` 合法 JSON 含 13 server |
| `doubao` | `.doubao/manual/{README.md, connectors.json, instructions.md, agents×15, skills×59}` | `connectors.json` 合法 JSON 含 13 server |

反向也验过：`--source dsh --target codex` 从 cordis 托管块读回，生成的 `config.toml` 13 个 server 全数还原。
详细命令、验收脚本与「实测暴露的 3 个缺陷」见 [docs/REAL_MIGRATION.md](docs/REAL_MIGRATION.md) §8–§10。

### 映射表平台化

映射表按 `source × target` 组合独立维护（`data/mappings/<source>-to-<target>.json`），每个平台组合可以有不同的语义判定（如 Trae 无 agents 概念 → `unsupported`；灵码（Qoder CN）mcp-settings.json 格式有差异 → `partial`）：

```text
data/mappings/
├── claude-to-qoder.json      ← Claude Code → Qoder (mcp: merge)
├── claude-to-trae.json       ← Claude Code → Trae (agents: unsupported; mcp: merge)
├── claude-to-lingma.json     ← Claude Code → Qoder CN 灵码 (instructions: AGENTS.md; mcp: partial)
├── claude-to-workbuddy.json  ← Claude Code → WorkBuddy (mcp: 用户级 merge; agents/memory: unsupported)
├── claude-to-dsh.json        ← Claude Code → DeepSeek Harness (mcp: cordis patch YAML; agents: unsupported)
├── claude-to-doubao.json     ← Claude Code → 豆包 (全部 manual-paste 手动包)
├── codex-to-qoder.json       ← Codex → Qoder (mcp: TOML→JSON merge)
├── codex-to-trae.json        ← Codex → Trae (agents: unsupported; mcp: merge)
├── codex-to-lingma.json      ← Codex → Qoder CN 灵码 (instructions: AGENTS.md; mcp: partial)
├── codex-to-workbuddy.json   ← Codex → WorkBuddy (mcp: 用户级 merge; agents/memory: unsupported)
├── codex-to-dsh.json         ← Codex → DeepSeek Harness (mcp: TOML→cordis YAML)
├── codex-to-doubao.json      ← Codex → 豆包 (全部 manual-paste 手动包)
└── 逆向 12 个: qoder/trae/lingma/workbuddy/dsh/doubao → claude / codex
```

解析顺序（优先级从高到低，全部在 `src/main.rs::load_mapping` 实现）：

1. `--mapping <path>` —— 显式指定，加载失败即 **fail-fast 中止**（绝不静默换表）。
2. `~/.migrator/mappings/<source>-to-<target>.json` —— 用户级覆盖，**与 CWD 无关**，发布后的单二进制靠它实现零代码扩展。
3. `data/mappings/<source>-to-<target>.json` —— 仓库内相对路径（仅在项目根运行 / 开发态命中）。
4. 编译期内嵌 JSON —— 单二进制自带全部 16 个组合，保证任意安装方式、任意 CWD 下行为一致（**分发正确性关键**）。
5. `MappingTable::builtin` —— 硬编码兜底，未知组合显式报错。

> **新增平台组合 = 加一个 JSON 文件，零代码改动**这一契约在发布后仍成立：
> 用户把 JSON 放进 `~/.migrator/mappings/`（层级 2）即可扩展，无需重编译源码。
> 至于「仓库内 JSON（层级 3）与内嵌快照（层级 4）的一致性」，由 `tests/` 集成测试
> 与 `converter::mapping` 单测的双份维护漂移守卫保证。

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
3. **Target Adapter** — 生成目标平台格式：`qoder` / `trae` / `lingma` / `workbuddy`
4. **安全写入层** — 托管块 v1、原子写入、备份/restore/clean、幂等

### 五安全契约（v1 冻结）

| 契约 | 实现 |
|---|---|
| 默认只读 | `apply` 必须显式 `--yes`；`--only` 限定配置面 |
| 托管块 | `<!-- BEGIN/END MIGRATOR:<scope> -->`，手写内容保护，标记异常拒绝写入 |
| 托管块 cutover | 当前 scope 缺失但文件里有旧契约 scope（`migrator:<source_path>`）的块 → **就地升级并收敛为一个**，绝不追加第二块 |
| 原子写入 | `.tmp` + rename，中断不留半写；fsync 在**写句柄**上做（Windows 只读句柄会 `ERROR_ACCESS_DENIED`） |
| 备份回退 | 写入前自动备份 → `restore`（演练）→ `clean` |
| 幂等 | 同一迁移重复执行结果一致 |

---

## 支持能力

### 已支持的配置面（ConfigSurface）

| 配置面 | Claude Code 源 | Codex 源 | 转换行为 |
|---|---|---|---|
| instructions | CLAUDE.md | AGENTS.md | → 目标平台规则（Qoder/Trae/灵码(现 Qoder CN)=项目根 AGENTS.md，WorkBuddy=rules/ 引用） |
| mcp | .mcp.json | config.toml [mcp_servers] | → 目标 MCP 文件（TOML→JSON；qoder/trae/workbuddy 为 merge 保留已有） |
| skills | .claude/skills/ | ~/.codex/skills/ | frontmatter 降级 + 子目录复制 |
| agents | .claude/agents/*.md | ~/.codex/agents/*.toml | YAML/TOML → Qoder/灵码(现 Qoder CN) .md 格式；Trae/WorkBuddy unsupported |
| commands | .claude/commands/*.md | — | → 单文件 skill（`skills/<name>/SKILL.md`，convert） |
| hooks | .claude/hooks/hooks.json | ~/.codex/hooks.json | 扫描报告（目标无等价物 → Not Added） |
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

> **发布后的扩展入口**：单二进制不含 `data/` 目录，但映射表已编译期内嵌（16 组合全自带）。
> 若要新增未在内置中的平台组合，把 JSON 放到**用户级覆盖目录**
> `~/.migrator/mappings/<source>-to-<target>.json` 即可，无需重编译（解析优先级高于内嵌表）。

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

测试：**120 个单元测试** + **4 个端到端集成测试**（`tests/cli_e2e.rs`，直接跑真实二进制，覆盖 scan/apply/backup/restore 全链路与安全契约），0 警告。
> 注：3 个既有 lib 单元测试（`atomic_write_creates_file` / `atomic_write_overwrites` / `target::qoder::write_creates_file_with_content`）因沙箱对系统临时目录的 `os error 5` 写/删保护而失败，与逻辑无关；集成测试已改用 `CARGO_TARGET_TMPDIR` 规避该雷区。

---

## 路线图

- [x] Phase 1: 四层管线骨架 + 托管块 v1 + 原子写入 + 映射表 schema v1
- [x] Phase 2: 语义降级 + MANUAL 块 / 备份回退链 / 映射表 JSON 化 / 记忆只读索引
- [x] 真实环境验证: Codex 源（45 skills + 15 agents + 10 MCP 全链路）
- [x] Phase 3: 多目标适配器（Qoder / Trae / 灵码(现 Qoder CN)）+ 平台化映射表
- [x] Phase 4: WorkBuddy 目标（用户级 MCP merge / 规则引用化 / agents·memory 降级）
- [x] Phase 5: commands→skills 转换 + hooks 扫描报告 + 报告显示目标路径
- [x] Phase 6: 双向迁移（国产 → Claude/Codex：LayoutSource 反向源 + claude/codex 布局 + JSON↔TOML 转换）
- [ ] 会话历史迁移
- [x] GitHub Actions CI（三平台 test + release 构建）

## License

MIT
