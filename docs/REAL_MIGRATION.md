# 真实迁移指南 — Codex → Qoder 实战

> 用 migrator 把 `~/.codex` 的真实配置（AGENTS.md / MCP / skills / agents / memories）完整迁移到 Qoder。
> 全程安全：默认只读 + 写入前自动备份 + 可回退。

---

## 0. 迁移前准备

```bash
# 1. 查看源侧配置全景（只读，安全）
migrator --source codex scan --project .

# 2. 确认目标平台版本（本指南以 Qoder 为例，Trae/灵码（现 Qoder CN）同流程，换 --target 即可）
migrator --source codex --target qoder plan --project .
```

### 迁移内容一览

| 配置面 | 源 (~/.codex) | 目标 (.qoder/) | 行为 |
|---|---|---|---|
| instructions | `AGENTS.md` (全局规则) | `AGENTS.md` | 托管块包裹，手写内容不覆盖 |
| mcp | `config.toml [mcp_servers]` | `.mcp.json` | TOML→JSON，嵌套 env 合并，HTTP server 保留 |
| skills | `skills/` (45 个) | `skills/` | frontmatter 降级 + references/scripts 子目录复制 |
| agents | `agents/*.toml` (15 个) | `agents/` | TOML→目标格式，`developer_instructions` 保留 |
| memory | `memories/` | `claude-memory-index/` | **只读索引**（正文留在 Codex 侧） |

---

## 1. 干跑预览（推荐先看）

```bash
# 完整预览迁移计划与语义降级判定（不写入任何文件）
migrator --source codex --target qoder dry-run --project .

# 输出示例
[映射表] 已加载: codex → qoder
[Added               ] instructions ~/.codex/AGENTS.md — convert
[Check before using  ] mcp ~/.codex/config.toml ([mcp_servers]) — mcp server zz-postgres-prod: env_vars [...] not mapped
[Check before using  ] skills ~/.codex/skills/mtkf-role-workflow/SKILL.md — fields downgraded: paths
[Not Added           ] hooks ~/.codex/hooks.json — Codex hooks differ from Qoder hook runtime
```

**怎么看这份报告**：
- `[Added]` → 迁移后即可用
- `[Check before using]` → 已写入，但字段有语义差异，**需要人工审查**（目标文件内嵌 `## MANUAL MIGRATION REQUIRED` 块说明差异）
- `[Not Added]` → 目标平台无对应物（如 hooks），只报告不迁移

---

## 2. 执行迁移

```bash
# 注意：必须显式 --yes（契约：默认只读）
migrator --source codex --target qoder apply --project . --yes
```

### 迁移产物结构

```
.qoder/
├── AGENTS.md              ← 托管块包裹你的全局规则
├── .mcp.json              ← 10 个 MCP server（TOML→JSON 转换）
├── skills/ (45 个)        ← frontmatter 降级 + 子目录完整复制
├── agents/ (9 个)         ← TOML 转换，developer_instructions 保留
└── claude-memory-index/   ← 只读索引（标题/路径/预览）
```

### 自动备份

每次 apply 前自动备份已存在的目标文件到 `.migrator-backups/<batch-id>/`：
- 手写进 AGENTS.md 的内容**不会被覆盖**（托管块保护）
- 迁移后想恢复 → 见第 4 步

---

## 3. 验收清单

```bash
# 1. 校验目标文件存在
migrator --source codex validate --project .

# 2. 确认 AGENTS.md 托管块完整（BEGIN/END 成对，手写内容在外层）
cat .qoder/AGENTS.md

# 3. 确认 MCP server 数量与源一致
# 源: grep -c "\[mcp_servers" ~/.codex/config.toml
# 目标: python -c "import json; print(len(json.load(open('.qoder/.mcp.json'))['mcpServers']))"

# 4. 抽查 skills 子目录是否复制完整（references/scripts/agents）
ls .qoder/skills/mtkf-role-workflow/

# 5. 确认 memory 索引生成且正文未复制
cat .qoder/claude-memory-index/README.md
```

---

## 4. 回退（如不满意）

```bash
# 列出备份批次
migrator backups --project .

# 演练恢复（只看计划，不写盘）
migrator restore --project .

# 执行回滚（恢复到最近备份）
migrator restore --project . --yes

# 清场（删除备份，用户文件保留）
migrator clean --project . --yes
```

---

## 5. 常见问题

| 问题 | 处理 |
|---|---|
| MCP 报 `env_vars not mapped` | Codex 特有字段，检查目标平台是否支持该 env 注入方式，必要时手动调整 |
| skill 报 `fields downgraded: model/paths` | 这些字段目标平台无等价物，已转为 MANUAL 提示，按提示手工补充 |
| agents 报 `sandbox semantics may differ` | Codex 的 sandbox_mode 与 Qoder 语义不同，审查 `developer_instructions` 是否完整 |
| hooks 显示 Not Added | 目标平台无 hook 机制或格式不同，需手工迁移 |
| 迁移后 Qoder 不识别 | 确认 Qoder 读取 `.qoder/` 目录（或用 `--target trae` / `--target lingma` 换平台重试） |

---

## 6. 迁移到其他平台

```bash
# Trae（无子 agent 概念 → agents 不迁移，只报告）
migrator --source codex --target trae apply --project . --yes

# 通义灵码（已更名 Qoder CN：项目根 AGENTS.md + mcp-settings.json）
# 规则输出到项目根 AGENTS.md（LINGMA.md 已弃用；IDE 侧规则另存 .lingma/rules/）
migrator --source codex --target lingma apply --project . --yes

# Qoder（AGENTS.md 放项目根，MCP merge 到 .qoder/.mcp.json）
migrator --source codex --target qoder apply --project . --yes
```

各平台差异与映射语义见 [README](../README.md#映射表平台化)。

---

## 7. 迁移到 WorkBuddy（腾讯 AI 编程产品）

```bash
# 干跑预览（推荐先看）
migrator --source codex --target workbuddy dry-run --project .

# 执行迁移
migrator --source codex --target workbuddy apply --project . --yes
```

### WorkBuddy 特有行为（实测 2026-08-17 校验）

| 配置面 | 迁移行为 | 说明 |
|---|---|---|
| instructions | 全文 → `~/.workbuddy/rules/codex-rules.md`；`MEMORY.md` 只保留托管块引用 | 避免数百行规则超出记忆容量限额（12KB > 4000 字符会截断） |
| mcp | TOML→JSON 后 **merge** 到用户级 `~/.workbuddy/mcp.json` | 官方路径（非 `.mcp.json`）；保留已有 server，同名以迁移内容为准 |
| skills | `~/.workbuddy/skills/`（45 个） | SKILL.md 格式兼容，frontmatter 需抽查 |
| agents | **不迁移**（Not Added） | WorkBuddy 无文件级 subagent（Expert 是市场概念） |
| memory | **不迁移**（Not Added） | WorkBuddy 无只读记忆索引消费方；正文留在 Codex 侧 |

### 验收清单

```bash
# 1. 规则全文落位
cat ~/.workbuddy/rules/codex-rules.md | head

# 2. MEMORY.md 托管块为引用（应 < 1KB，而非 400+ 行规则）
wc -l ~/.workbuddy/MEMORY.md

# 3. MCP 合并到官方路径
python -c "import json; print(len(json.load(open(r'~/.workbuddy/mcp.json'.replace('~', __import__('os').path.expanduser('~'))))['mcpServers']))"

# 4. skills 数量与源一致
ls ~/.workbuddy/skills/ | wc -l
```

> **注意事项**：WorkBuddy 的 MCP 配置若由客户端/连接器运行时管理，新增 server 需在「连接管理」中确认启用；`.mcp.json`（带点前缀）不是官方配置路径，迁移不会写入该文件。

---

## 8. 迁移到 dsh（DeepSeek Harness）

```bash
# 干跑预览
migrator --source codex --target dsh dry-run --project .

# 执行迁移
migrator --source codex --target dsh apply --project . --yes

# 校验
migrator --source codex --target dsh validate --project .
```

### dsh 特有行为（实测 2026-10-26 校验，本机真实客户端）

| 配置面 | 迁移行为 | 说明 |
|---|---|---|
| instructions | `~/.codex/AGENTS.md` → `~/.dsh/AGENTS.md`（托管块包裹） | 两者都是用户级全局规则，不需要项目目录 |
| mcp | `~/.codex/config.toml [mcp_servers]` → `~/.dsh/profiles/<profile>/cordis.patch.yml` 的 **cordis 插件条目** | dsh 的 MCP **不是 JSON**，而是 cordis loader 的 patch 数组；条目形如 `- id: mcp-<name>` / `name: '@deepseek-ai/dsh-mcp-client'` / `config: {serverName, transport, command, args, env, cwd, url, headers}` |
| skills | `~/.dsh/skills/<name>/SKILL.md`（实测 59 个） | 与 `~/.agents/skills`、`<project>/.dsh/skills` 同级候选路径 |
| agents | **不迁移**（Not Added） | dsh 无文件级 subagent，由 preset / profile 驱动 |
| memory / hooks | **不迁移**（Not Added） | dsh 无对应消费方 |

### profile 选择策略

`cordis.patch.yml` 位于 `~/.dsh/profiles/<profile>/` 下。迁移按以下顺序定 profile，不猜：

1. 存在 `profiles/desktop/` → 用它（桌面客户端的默认 profile）
2. 否则取**第一个**实际含 `cordis.patch.yml` 的 profile 目录
3. 都找不到 → 退回 `desktop`（并自动建目录）

### 写入约定（保用户数据）

- 只 upsert `# <!-- BEGIN MIGRATOR:dsh:mcp -->` … `# <!-- END MIGRATOR:dsh:mcp -->` 之间的注释托管块
- 块外条目、注释、`!!js` 表达式**逐字保留**（该文件解析再序列化会摧毁用户表达式，因此全部走字符串处理）
- 块外已存在同名 `id: mcp-<name>` → 跳过该 server 并记入报告（dsh 遇重复 id 会加载失败）
- 环境变量占位 `{env:FOO}` → `!!js 'process.env.FOO'`；内嵌占位 → `!!js '`…`'` 模板字符串
- YAML 单引号包裹是**语法必需**：反引号是 YAML 保留指示符，裸写 ``!!js `…` `` 直接解析报错

### 验收清单

```bash
# 1. 规则托管块存在（BEGIN/END 各 1）
grep -c "MIGRATOR" ~/.dsh/AGENTS.md

# 2. cordis patch 语法合法 + id 唯一 + 块内条目数
python -c "import yaml,os;d=yaml.load(open(os.path.expanduser('~/.dsh/profiles/desktop/cordis.patch.yml'),encoding='utf-8'),Loader=yaml.BaseLoader);print(len(d),'entries')"

# 3. server 数与本工具 validate 一致
migrator --source codex --target dsh validate --project . | grep mcp

# 4. 幂等：再跑一次 apply，文件 md5 不应变
md5sum ~/.dsh/profiles/desktop/cordis.patch.yml
```

---

## 9. 迁移到豆包（Doubao 桌面客户端）— 手动迁移包

```bash
migrator --source codex --target doubao apply --project . --yes
```

### 为什么是「手动包」而不是直接写入

豆包桌面客户端**没有文件级配置契约**。实测 `%LOCALAPPDATA%\Doubao\User Data\Default\Local Storage\leveldb`
里只能看到 `samantha-web-agent-connector-list-store` 之类的键，MCP「自定义连接器」、技能、规则
全部存在客户端内部存储，**只能由用户在界面粘贴**。

因此 `--target doubao` 不假装能写配置，而是产出一个可粘贴的迁移包 `{project}/.doubao/manual/`：

| 文件 | 用途 |
|---|---|
| `README.md` | 本次迁移清单 + 逐步粘贴指引（自动生成，含每条配置面的处理状态） |
| `connectors.json` | `mcpServers` 结构，逐个 server 粘进「+ 添加 → 新建自定义连接器」 |
| `instructions.md` | 粘到豆包的「自定义指示 / 人设 / 规则」输入框 |
| `agents/*.md` | 15 个 subagent，按内容在客户端手动新建或作为对话附件使用 |
| `skills/` | 59 个 skill，豆包技能走技能市场，无法文件导入 |

> 豆包目标下 instructions **不加**托管块标记：产物是给人整段复制的，`<!-- BEGIN MIGRATOR -->`
> 只会变成噪音。

### 反向：从豆包读回

`--source doubao` 读取的就是上面这个手动包目录（`{project}/.doubao/manual/`），
可以继续迁到 Claude / Codex / dsh，不会去碰 leveldb。

---

## 10. 三个国产目标真机实测记录（2026-10-26）

同一份 `~/.codex` 源，三个目标全部实跑，非纸面实现：

| 目标 | 产物 | 断言结果 |
|---|---|---|
| `dsh` | `~/.dsh/AGENTS.md`、`~/.dsh/profiles/desktop/cordis.patch.yml`、`~/.dsh/skills/`×59 | YAML 语法合法；19 条条目（用户原有 6 + MCP 13），id 唯一；重复 apply 文件 md5 不变 |
| `workbuddy` | `~/.workbuddy/mcp.json`、`MEMORY.md`（引用块）、`rules/codex-rules.md`、`skills/`×59 | `mcp.json` 合法 JSON 含 13 server；`MEMORY.md` 仅 394 字节引用 |
| `doubao` | `.doubao/manual/{README.md, connectors.json, instructions.md, agents/×15, skills/×59}` | `connectors.json` 合法 JSON 含 13 server |

反向验证：`--source dsh --target codex` 读回 cordis 托管块 → 生成合法 `config.toml`，
13 个 server 全数还原（`bearer_token_env_var` 从 `Authorization` 头反推、`node_repl` 18 个 env 保留）。

### 实测暴露并修复的三个缺陷

| 缺陷 | 症状 | 根因 | 修法 |
|---|---|---|---|
| 只读句柄 fsync | Windows 上任何 `apply` 都 `拒绝访问 (os error 5)`，只留下 `.tmp` | `fs::write` 后再 `File::open`（只读句柄）调 `sync_all()`；Windows `FlushFileBuffers` 要求写权限。Unix 的 fsync 对只读 fd 有效，所以在 Linux 上测不出来 | 改用 `File::create` 拿写句柄，同一句柄上 `write_all` + `sync_all` |
| match 分支顺序 | `codex → dsh` 退出码 0、文件 mtime 变了，但 MCP 配置**静默丢失**（托管块一条没有） | `convert_surface` 里 `("codex", _)` 排在 `(_, "dsh")` 之前，命中即返回，YAML 转换器根本没被调用 | 具体组合前置（新增 `("codex","dsh")` 接力分支 + `("codex","codex")` 直通），并加回归测试锁死顺序 |
| 托管块块中块 | `dsh → codex` 后目标 `AGENTS.md` 出现 **4 个** MIGRATOR 标记（块中块），来回迁移会无限套娃 | 源端读取 instructions 时原样搬运了本工具自己的标记 | 新增 `managed_block::strip_markers`，读取 instructions 时整行剥离标记，正文完整保留 |

---

## 11. 只迁移单个配置面（`--only`）

```bash
# 只同步规则，完全不碰 MCP（推荐用于「改规则」这类日常操作）
migrator --source codex --target workbuddy --only instructions apply --project . --yes

# 只同步 MCP 与技能
migrator --source codex --target dsh --only mcp,skills apply --project . --yes

# 只校验某个面
migrator --source codex --target workbuddy --only instructions validate --project .
```

支持的面：`instructions` / `mcp` / `skills` / `agents` / `commands` / `hooks` / `memory`。
面名拼错会 **fail-fast** 并列出可选项，不会静默忽略。

### 为什么需要它

写 MCP 会让真实客户端**重载连接器**（可能弹授权、刷新当前会话的 MCP 连接）。
只想更新几十行规则时被迫连带写 MCP，是很实际的痛点 —— 本项目的托管块 cutover 修复
就是被这个缺口卡住才补的。

实测断言：`--only instructions` 后 `~/.workbuddy/mcp.json` 的 md5 保持不变。

---

## 12. 托管块 scope 升级（cutover）

v0.2 早期版本的托管块 scope 是 `migrator:<source_path>`（如
`<!-- BEGIN MIGRATOR:migrator:~/.codex/AGENTS.md -->`），后来改为稳定的
`migrator:<target>:<kind>`。**旧版本写过的文件里会残留旧 scope 的块**。

当前版本对它做 **cutover**：

| 情况 | 行为 |
|---|---|
| 当前 scope 的块存在 | 正常重写块内内容（幂等） |
| 当前 scope 缺失，但存在旧 scope 块 | **就地升级**：在原位置把标记改成新 scope、内容换成新内容；`first_write=false` |
| 存在多个历史块（曾叠加过） | 先删掉其余块，再把第一个升级为新 scope —— **收敛为一个** |
| 完全没有 MIGRATOR 块 | 追加到文件末尾（`first_write=true`） |

### 验证方法

```bash
# 升级前：旧 scope
grep -n MIGRATOR ~/.workbuddy/MEMORY.md
# <!-- BEGIN MIGRATOR:migrator:~/.codex/AGENTS.md -->

migrator --source codex --target workbuddy --only instructions apply --project . --yes

# 升级后：新 scope，位置不变，手写内容完整
grep -n MIGRATOR ~/.workbuddy/MEMORY.md
# 9:<!-- BEGIN MIGRATOR:migrator:workbuddy:instructions -->
# 13:<!-- END MIGRATOR:migrator:workbuddy:instructions -->
```

实测结果：块位置不变（第 9–13 行）、手写 11 行跨项目记忆逐字保留、`mcp.json` 零改动、
重复执行 md5 不变。

> 若 `validate` 报「检测到旧版 scope 托管块(…)」，说明文件还是旧契约状态，
> 跑一次对应 `apply` 即可自动升级，**不需要手工编辑**。
