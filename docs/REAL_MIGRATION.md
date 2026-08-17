# 真实迁移指南 — Codex → Qoder 实战

> 用 migrator 把 `~/.codex` 的真实配置（AGENTS.md / MCP / skills / agents / memories）完整迁移到 Qoder。
> 全程安全：默认只读 + 写入前自动备份 + 可回退。

---

## 0. 迁移前准备

```bash
# 1. 查看源侧配置全景（只读，安全）
migrator --source codex scan --project .

# 2. 确认目标平台版本（本指南以 Qoder 为例，Trae/灵码同流程，换 --target 即可）
migrator --source codex --target qoder plan --project .
```

### 迁移内容一览

| 配置面 | 源 (~/.codex) | 目标 (.qoder/) | 行为 |
|---|---|---|---|
| instructions | `AGENTS.md` (全局规则) | `AGENTS.md` | 托管块包裹，手写内容不覆盖 |
| mcp | `config.toml [mcp_servers]` | `.mcp.json` | TOML→JSON，嵌套 env 合并，HTTP server 保留 |
| skills | `skills/` (45 个) | `skills/` | frontmatter 降级 + references/scripts 子目录复制 |
| agents | `agents/*.toml` (9 个) | `agents/` | TOML→目标格式，`developer_instructions` 保留 |
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

# 通义灵码（LINGMA.md + mcp-settings.json）
migrator --source codex --target lingma apply --project . --yes
```

各平台差异与映射语义见 [README](../README.md#映射表平台化)。
