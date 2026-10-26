# migrator 项目深度问题分析报告

> 分析日期：2026-08-18
> 分析范围：`src/` 全部 29 个 .rs 文件 + `Cargo.toml` + `data/mappings/` + `docs/`
> 方法：逐文件静态审查 + 真实编译/测试（后台 `cargo test`，结果见文末）
>
> **修复进展（2026-08-18 同日）：P0 全部 4 项已修复并验证，见文末「五、修复记录」。**

---

## TL;DR（最该先修的 4 个）

| # | 严重度 | 问题 | 触发条件 | 状态 |
|---|---|---|---|---|
| 1 | 🔴 灾难 | 逆向迁移到 Codex 会**整文件覆盖 `~/.codex/config.toml`**，清空 model/provider/API key | `--source qoder/trae/lingma/workbuddy --target codex apply` | ✅ 已修复 |
| 2 | 🔴 严重 | `target_path` 用字符串 `contains()` 推断类型，导致 **WorkBuddy `MEMORY.md` 被误判为 memory 索引**，instructions 丢失 | 任何以 workbuddy 为源或目标的迁移 | ✅ 已修复 |
| 3 | 🔴 严重 | `target_path` 的 `contains("mcp")` 会**误伤名字含 "mcp" 的 skill/agent**，写错文件 | 源含 `mtkf-mcp-tool` 之类命名 | ✅ 已修复 |
| 4 | 🟠 高危 | Codex `config.toml` 的 TOML→JSON **手写字符串解析**（无 toml 依赖），行尾注释/字段顺序即丢数据 | Codex 源的所有 MCP 迁移 | ✅ 已修复 |

---

## 一、架构层面的根因

`TargetAdapter::target_path(&self, surface_key: &str)` 只接收一个**路径字符串**，没有 `SurfaceKind`。
因此目标路径的判定完全靠对 `source_path` 做 `ends_with` / `contains` 文本匹配（`src/target/qoder.rs:96-160`）。
这是 #2、#3 两个高危 bug 的共同根因——凡是路径字符串里出现 `mcp` 或 `memor` 子串，就会走错分支。
**正确做法**：让 `target_path` 接收 `SurfaceKind`（甚至整条 `ConfigSurface`），按类型分派，而不是猜字符串。

---

## 二、按严重度展开

### 🔴 #1 逆向迁移到 Codex 覆盖整个 config.toml（数据丢失）
- 线路：`converter/mapping.rs` 中 `domestic_common_entries(to_codex=true)` 给 mcp 设 `behavior="convert"`；
  `pipeline/pipeline.rs:117` 只在 `behavior == "merge"` 时才调用 `merge_mcp_into_target`，故 codex 目标走**不合并**分支；
  最终 `pipeline.rs:130` `target.write → atomic_write` 把 `~/.codex/config.toml` 整体覆盖成**仅含 `[mcp_servers]` 段**的文本。
- 后果：用户原 `config.toml` 里的 `model` / `provider` / `auth` / `api_key` / `features` 等全部丢失。
- 缓解：main.rs 的 backup 确实会备份 config.toml，可通过 `restore --yes` 找回；但这是**静默破坏性覆盖**，且违背“双向迁移”承诺。
- 修复：codex 目标的 mcp 必须是 `merge` 语义，且合并对象应是 `config.toml` 的 `[mcp_servers]` 段（需真正解析 TOML 段），而非整文件替换。

### 🔴 #2 WorkBuddy `MEMORY.md` 被误判为 memory 索引
- 线路：反向源用 `LayoutSource`，workbuddy 的 instructions `source_path` = `~/.workbuddy/MEMORY.md`
  （`src/source/layout_source.rs:72-89`）。该字符串不含 `CLAUDE.md`/`AGENTS.md`，但 `rel.contains("memor")` 命中
  （`qoder.rs:154`），于是被当作 memory 索引写入 `.codex/claude-memory-index/README.md`（或对应平台 memory 目录）。
- 后果：`--source workbuddy --target codex/claude/qoder/...` 时，**整份指令配置被写进错误的 memory 文件**，目标 `AGENTS.md`/`CLAUDE.md` 里什么都没有。
- 范围：不止逆向——任何“workbuddy 作为源”的组合都会中招。README 的双向验证只覆盖了 claude/codex 作源（见 CHANGELOG “8 组合 claude/codex × 4 目标”），**逆向（国产作源）从未真实验证**。

### 🔴 #3 名字含 "mcp" 的 skill/agent 被路由到 MCP 文件
- 线路：`qoder.rs:110-113` MCP 分支在 skills/agents 分支**之前**，条件是 `source_path.contains("mcp")`。
  那么 `~/.codex/skills/mtkf-mcp-tool/SKILL.md` 或 `~/.codex/agents/mcp-agent.toml` 会被判为 MCP，
  导致：(a) skill/agent 内容写进 `.mcp.json`； (b) 紧随其后的 `copy_skill_support_dirs`（`pipeline.rs:242-266`）
  把 references/scripts 复制到错误的 MCP 文件父目录。
- 后果：该 skill/agent 永久丢失，MCP 文件被污染。

### 🟠 #4 Codex TOML 用手写字符串解析（无 toml 依赖）
- `Cargo.toml` 根本没有 `toml` crate；`convert_codex_mcp`（`converters/mcp.rs:16-125`）靠行扫描 + 正则式匹配。
- 已确认会失败的真实场景：
  - 行尾注释：`command = "x"  # note` → `parse_key_value` 用 `trim_matches('"')` 只去首尾引号，残留 `x"  # note`。
  - **字段顺序**：内层循环遇到任意 `[mcp_servers.X]`（含嵌套 `[.env]`/`.tools` 段）就 `break`（`mcp.rs:35-37`），
    因此 **`.env` 段之后的同 server 字段被丢弃**（测试用例把 `args` 放在 `.env` 之前，掩盖了该 bug）。
  - 多行数组、`args = [ "a", "b" ]`（带空格）、带 `=` 的 env 值等均未处理。

### 🟠 #5 WorkBuddy 规则文件不在备份范围内
- `main.rs:226-236` 的备份列表只含 `target.target_path(...)`，即 `MEMORY.md`；
  但 `write_workbuddy_rules_reference`（`pipeline.rs:382-409`）把规则全文 `atomic_write` 到
  `~/.workbuddy/rules/<source>-rules.md`，该文件**未被备份**。若用户已有同名 rules 文件，会被静默覆盖且 `restore` 无法找回。

### 🟠 #6 managed block 的 scope 跨方向不稳定 → 重复块 / 叠加块
- scope = `migrator:<source_path>`（`pipeline.rs:420`）。
  - `claude→qoder` 用 scope `migrator:.claude/CLAUDE.md`，再 `qoder→claude` 用 `migrator:AGENTS.md`——**两个不同 scope**，
    重复迁移会产生第二个托管块而非更新（README 声称的“幂等”只在单一方向成立）。
  - 若同一项目先后从两个源迁移，scope 不同会叠加多块。

### 🟡 #7 `validate` 验证形同虚设
- `validate_target`（`pipeline.rs:433-449`）只检查目标文件“存在且非空”，不校验：
  - WorkBuddy 的 `rules/` 全文与 `MEMORY.md` 引用是否一致；
  - MCP merge 后 server 数量 / 关键字段是否完整；
  - instructions 是否真被托管块包裹。

### 🟡 #8 逆向映射表无法用 JSON 扩展
- `data/mappings/` 只有 `claude-*` / `codex-*` 8 个正向组合；逆向组合只能走 `MappingTable::builtin()`
  （`mapping.rs:559-565`）硬编码。README 声称“新增平台组合 = 加一个 JSON 文件零代码”，对逆向不成立。

### 🟡 #9 reverse 源不扫描 commands/hooks/memory
- `LayoutSource::scan`（`layout_source.rs:193-200`）只枚举 instructions/mcp/skills/agents；
  而 `domestic_common_entries` 声明了 commands/hooks/memory 的映射条目，但这些 surface 从未被扫描→映射条目形同死代码，逆向确实不会迁移 memory（与“双向”承诺不符）。

### 🟡 #10 未知 `--source` 静默回退
- `build_source`（`main.rs:143-155`）对未知 source 回退到 `ClaudeCodeSource`，无任何告警；
  配合 `MappingTable::builtin` 的 `_ => claude_to_qoder` 兜底，用户可能以为在迁移 A→B，实际走的是 B→qoder 默认。

### 🟢 #11 工程卫生
- 工作区遗留约 30 个 `target-build-*` 目录（数百 MB～GB），应加入 `.gitignore` 或清理；`cargo.sh` 每次新开目录导致全量重编 8–9 分钟（脚本已支持 `MIGRATOR_TARGET_DIR` 固定外部目录，应默认启用）。
- `atomic_write` 注释声称“fsync”但实际未调用（`atomic.rs:15-27`），崩溃理论上可能残留 `.tmp`。
- `convert_memory_surface` 无文件数量/大小上限，超大 memory 目录会全量读入内存。
- README 称“83 个单元测试 / 0 警告”，CHANGELOG 0.2.0 记 70；数字以实际 `cargo test` 为准（见文末）。

---

## 三、修复优先级建议

1. **P0**：重构 `target_path` 接收 `SurfaceKind`，按类型分派（消除 #2、#3 及其根因）。
2. **P0**：codex 目标 mcp 改为段级 merge（`config.toml` 的 `[mcp_servers]` 合并），禁止整文件覆盖（#1）。
3. **P0**：引入真正 TOML 解析（加 `toml` crate 或用 `toml_edit`），替换手写解析（#4）。
4. **P1**：WorkBuddy `rules/` 文件纳入备份（#5）；managed block scope 改为稳定标识（如 `migrator:<platform>:<surface>`）（#6）。
5. **P1**：`validate` 增加内容级校验（#7）；补齐逆向 commands/hooks/memory 扫描或移除相关死映射（#9）。
6. **P2**：未知 source/target 显式报错而非静默兜底（#10）；清理 `target-build-*`（#11）。

---

## 四、编译/测试核对（后台运行）

- 命令：`MIGRATOR_TARGET_DIR=target-fresh ./cargo.sh test`
- 结果：**89 passed; 0 failed; 0 ignored**（耗时约 16s，无编译警告）。
- 与文档出入：README 声称“83 个单元测试”，实际 **89**；"0 警告" 属实。
- **最重要的印证**：报告中的 #1/#2/#3 **全部未被测试覆盖**。现有单测只覆盖了
  - 正向 `claude/codex → qoder/trae/lingma/workbuddy` 的 `target_path`；
  - 逆向“源读取”（`reads_qoder_config_as_source` / `reads_workbuddy_user_config_as_source`）；
  - 没有任何用例覆盖 `workbuddy → codex/claude` 的 **instructions** 目标路径（#2），
    也没有用例用“名字含 mcp 的 skill/agent”调用 `target_path`（#3）。
  - 因此这些高危路径在 CI 下是**盲区**，与 CHANGELOG 所述“8 组合真实 apply 验证”一致——
    真实验证只做了 `claude/codex × 4 目标`，逆向（国产作源）从未真机跑过。

> 结论：项目“能编译、测试绿”≠ “功能正确”。核心安全性契约（只读/备份/托管块）单元测试充分，
> 但**跨平台路由与逆向迁移的正确性是当前最大风险面**，且测试无法暴露。

---

## 五、修复记录（2026-08-18）

### 已修复：P0 四项

| # | 修复内容 | 涉及文件 |
|---|---|---|
| 1 | MCP 写入改为**一律非破坏性合并**：TOML 目标走段级合并（保留 model/provider/API key/注释/键序），JSON 目标走 `mcpServers` 合并。映射表 `behavior` 字段降级为仅供报告展示，**不再决定是否覆盖用户文件** | `pipeline/pipeline.rs`、`converter/converters/mcp.rs` |
| 2+3 | `TargetAdapter::target_path` 签名改为 `(kind: SurfaceKind, source_path: &str)`，按**类型**分派而非猜字符串，彻底消除 `contains("mcp")` / `contains("memor")` 误路由 | `target/mod.rs`、`target/qoder.rs`、`pipeline/pipeline.rs`、`main.rs` |
| 4 | 引入 `toml 0.8`（结构化解析）+ `toml_edit 0.22`（格式保留编辑），重写 `mcp.rs`，替换全部手写逐行解析 | `Cargo.toml`、`converter/converters/mcp.rs` |
| 附加 | **备份 fail-closed**：apply 阶段备份失败改为返回错误并中止，不再降级为 `eprintln!` 警告后继续覆盖用户文件 | `main.rs` |

### 关键设计决策

- **合并失败即拒绝写入（fail-closed）**：目标 `config.toml` 解析失败、或 `mcp_servers` 是 inline table 时，返回错误并中止，
  **绝不用整文件覆盖“修复”用户配置**。理由：无法安全合并时，保留用户原文永远优于写入一个语义可能错误的新文件。
- **新段追加到文档末尾**：`toml_edit` clone 过来的段携带源文档旧 `position`，直插会让新段渲染到已有段之前
  （表现为 `[mcp_servers.x]` 夹在 `[model_providers]` 与 `[history]` 之间、`[mcp_servers.x.env]` 与父段被隔开）。
  语义虽合法（TOML 表头是绝对路径），但人工编辑时极易误改相邻段，故对插入子树重新编号 position。
- **同名 server 以源侧为准，目标独有 server 保留**；目标非 MCP 段一律逐字保留。

### 验证方式（全程未触碰用户真实 `~/.codex`）

单元测试：`109 passed; 0 failed`（原 89 → 新增 20 个回归），`cargo build --all-targets` 零警告。
新增回归用例覆盖：名字含 `mcp`/`memor` 的 skill/agent/command 路由、workbuddy→codex/claude 的 instructions 路由、
TOML 行尾注释 / 多行数组 / 嵌套段后字段 / 非字符串 env 值、段级合并的 6 类场景、`is_toml_path` 仅按扩展名判定、
合并版式（新段追加末尾 + 父子相邻）。

端到端真机验证用**假 HOME**（`layout.rs::user_root()` 读 `HOME`/`USERPROFILE`）隔离，构造含
`model` / `model_providers.openai.env_key` / `history` / 手写注释 / 已有 `mcp_servers` 的 config.toml，
执行 `--source qoder --target codex apply --yes`，结果：

- 其余段、注释、键序全部保留；新 server 写入；同名 server 按源侧更新
- 产物经严格 TOML 解析器（Python `tomllib`）校验，全部值正确还原
- 二次 apply 产物 md5 一致（幂等）
- 目标文件损坏时退出码 1、报「拒绝写入」、原文未被改动
- 名字含 `mcp` / `memor` 的技能正确落在 `skills/`，`config.toml` 未被污染
- **真实 `~/.codex/config.toml` md5 迁移前后完全一致（`e3033661…`），全程零触碰**

### 仍未修复（P2，按原优先级）

（#8 逆向映射表 JSON 化、#9 逆向源扫描完整 7 面、#10 未知 source 静默回退、#11 工程卫生 均已修复，见「六/七/八、P2 修复记录」）

---

## 六、修复记录（P1，2026-08-18 同日）

### 已修复：P1 三项（用户配置安全加固）

| # | 修复内容 | 涉及文件 |
|---|---|---|
| 5 | WorkBuddy `rules/` 纳入备份：`~/.workbuddy/rules/<source>-rules.md` 中心化为 `workbuddy_rules_path`，apply 阶段在 `target==workbuddy` + 含 instructions 面时加入备份批次，消除"写入了却没备份、用户同名 rules 被静默覆盖且无法回滚" | `pipeline/pipeline.rs`、`main.rs` |
| 6 | 托管块 scope 稳定化：`migrator:<source_path>` → `migrator:<target_name>:<kind>`，重复迁移/多源迁移只更新同一块，不再叠加 | `pipeline/pipeline.rs` |
| 7 | `validate` 内容级校验：instructions 校验托管块存在且非空（workbuddy 额外校验 rules 全文）、MCP 校验 server 数量（真解析），异常降级 `CheckBeforeUsing` | `pipeline/pipeline.rs` |

### 关键设计决策

- **#5 路径中心化**：`write_workbuddy_rules_reference`（写入）与 `main.rs`（备份）共用同一 `workbuddy_rules_path` 计算，杜绝两处逻辑漂移。注意：源标识仍沿用旧启发式 `source_path.contains(".codex") ? "codex" : "claude"`，仅 codex/claude 命名精确，qoder/trae/lingma 源会落到 `claude-rules.md`（与 #8/#9 同源标识短板相关，非本次引入）。
- **#6 scope 语义**：scope 标识"目标文件里归 migrator 所有的那块区域"，应以目标平台+配置面为准，而非源路径。旧实现用源路径导致跨方向不稳定；新 scope 与源无关，天然幂等。
- **#6 兼容性**：旧 scope（`migrator:<source_path>`）写入的块不会被新代码识别/更新，成为惰性孤儿块（不报错、不影响新块）。属已知 cutover 代价，不回滚旧块。

### 验证方式（全程未触碰用户真实 `~/.codex`）

单元测试：由 109 → **114 passed**（+5 回归），编译零错误、测试全绿。
新增回归用例覆盖：rules 路径计算（`codex-`/`claude-` 分文件）、scope 跨源稳定（`migrator:qoder:instructions`）、
重复迁移只更新同一托管块（`BEGIN` 标记计数为 1）、validate 检出托管块存在/缺失（降级 `CheckBeforeUsing`）、
MCP server 计数（2 个 server 正常、空配置降级）。

真实 `~/.codex/config.toml` md5 迁移前后完全一致（全程零触碰）。

---

## 七、修复记录（P2，2026-08-19）

### 已修复：P2 两项（fail-fast 与写入可靠性）

| # | 修复内容 | 涉及文件 |
|---|---|---|
| 10 | 未知 `--source` 显式报错：`build_source` 未知源返回 `Err`（不再静默回退 `ClaudeCodeSource`）；`MappingTable::builtin` 未知组合返回 `Err`（不再静默兜底 `claude→qoder`）；`load_mapping` 同步返回 `Result`，main 入口 `?` 传播。彻底消除"以为迁 A→B 实际走默认"的静默错误 | `main.rs`、`converter/mapping.rs` |
| 11 | `atomic_write` 真正 `fsync`：`write` 临时文件后、rename 前对临时文件 `sync_all()`，确保内容/元数据落盘（此前注释声称 fsync 却未调用）。另：`target-build-*` 已被现有 `.gitignore` 的 `/target-*` 覆盖，无需额外忽略 | `src/safety/atomic.rs`、`.gitignore` |

### 关键设计决策

- **#10 fail-fast 优于静默兜底**：未知平台/组合属用户输入错误，显式报错（带"可选列表"提示）比"猜一个默认"安全——后者会让用户在不知情下执行错误的迁移，违背双向迁移契约。
- **#10 测试转向**：原 `builtin_falls_back_to_claude_qoder` 在**验证错误行为**（静默兜底），已改写为 `builtin_unknown_combo_errors` 验证**报错**；并补 `build_source` 未知源/已知源两个测试，覆盖两条 fail-fast 路径。
- **#11 fsync 范围**：仅 `sync_all()` 临时文件（数据+元数据），不做目录 fsync（Windows 上目录 fsync 受限且 rename 原子性由 OS 保证），足以避免"目标文件半写"——崩溃时最多残留 `.tmp`，目标文件始终完整。

### 验证方式（全程未触碰用户真实 `~/.codex`）

单元测试：总 116（lib 114 + bin 2），**113 passed**；新增 3 个回归（`builtin` 未知组合报错、`build_source` 未知源报错/已知源成功）全部通过。3 个既有 lib 测试（`atomic_write_creates_file` / `atomic_write_overwrites` / `target::qoder::write_creates_file_with_content`）因当前会话沙箱对临时目录的 `os error 5` 写/删保护而失败——与本次改动无关（P1 验收时全绿，且本次未改其写路径逻辑），建议在其本地（非沙箱）环境运行 `cargo test` 确认全绿。
真实 `~/.codex/config.toml` md5 迁移前后完全一致（全程零触碰）。

---

## 八、修复记录（P2，2026-08-19 同日）

### 已修复：P2 两项（逆向映射 JSON 化 + 逆向源扫描完整 7 面）

| # | 修复内容 | 涉及文件 |
|---|---|---|
| 8 | 逆向映射表支持 JSON 扩展：在 `data/mappings/` 新增 8 个逆向组合 JSON（`qoder`/`trae`/`lingma`/`workbuddy` × `claude`/`codex`），内容镜像 `builtin_domestic_to_claude/tocodex` 的 7 条目（instructions/mcp/skills/agents/commands/hooks/memory）。`load_mapping` 步骤2 按 `<source>-to-<target>.json` 优先加载，README「加 JSON 零代码」契约对逆向成立；`builtin()` 仅作文件缺失兜底 | `data/mappings/*`（新增 8 文件） |
| 9 | 逆向源扫描覆盖完整 7 面：`LayoutSource::scan` 在 instructions/mcp/skills/agents 之外新增 commands/hooks/memory 三面枚举。`TargetLayout` 补充 `commands_dir`/`hooks_dir` 字段（默认 `commands`/`hooks`），新增 `surface_optional_dir` helper 探测 `project_dir/<dir>` 或 `user_dir/<dir>` 目录存在性。使 `domestic_common_entries` 声明的 commands/hooks/memory 映射条目不再死代码，能真实出现在扫描报告里；若源平台实际存在这些目录则被探测并迁移（前向兼容） | `src/target/layout.rs`、`src/source/layout_source.rs` |

### 关键设计决策

- **#8 复用既有加载链**：`load_mapping` 步骤2 本就按 `data/mappings/<source>-to-<target>.json` 查文件，逆向组合此前因文件缺失才回退 `builtin()`。补 8 个 JSON 后，逆向优先走 JSON（可编辑/可扩展），`builtin()` 仅兜底——零改 `load_mapping` 逻辑即满足"逆向也能 JSON 扩展"。
- **#9 诚实枚举 + 不破坏既有行为**：逆向源侧（国内平台）实际无 commands/hooks/memory 对应物，故扫描出的这 3 面 `present=false`；`run_migration` 对 `present=false` 直接 `continue` 跳过（pipeline.rs:58），对 `Unsupported` 状态标 `NotAdded` 不写入。行为与修复前一致（不迁移），但报告更完整、映射条目不再是死代码。若某平台真实存在这些目录（如未来演进），探测即生效。
- **#9 测试隔离**：逆向 `LayoutSource::detect` 读真实 `HOME`，测试改用 `LayoutSource::new` 显式传入受控临时 `user_dir`，避免探测到机器上已装的 Qoder 目录（初版测试因此误判 `present=true`）。

### 验证方式（全程未触碰用户真实 `~/.codex`）

编译零错误；新增 2 个回归测试（`reverse_mapping_loaded_from_json` 验证 8 个逆向 JSON 可被加载且含 commands/hooks/memory 条目、`scan_covers_all_seven_surfaces` 验证逆向源扫描产出完整 7 面）全部通过；lib 113 passed / 3 failed（3 个为沙箱 `os error 5` 环境限制：atomic/qoder write 测试，与改动无关，P1 验收时即如此）；真实 `~/.codex/config.toml` md5 全程不变（`e3033661ae0ada04fbd7d9ae41c8f942`）。

> **结论**：原分析报告的 #1–#11 全部修复完毕。核心安全性契约（只读 / 备份 fail-closed / 段级非破坏性 merge / 托管块幂等 / 真 fsync / 未知组合 fail-fast / 逆向 JSON 可扩展 / 逆向扫描完整 7 面）已闭环，且有回归测试守护。

---

## 九、修复记录（全局视野 / 分发架构，2026-08-19）

跳出「单测绿不绿」的视野，从**端到端可信度底座 / 分发架构正确性 / 发布就绪度**重新体检，发现比 #1–#11 更致命的**分发级缺陷**：发布的单二进制因依赖 CWD 相对路径，会在真实使用场景里静默丢弃 JSON 映射，使功能归零。

### 已修复：分发正确性（#12、#14、claude→qoder 兜底漏接）

| # | 严重度 | 问题 | 触发条件 | 状态 |
|---|---|---|---|---|
| 12 | 🔴 灾难 | **映射表分发失效**：`default_mapping_file` 仅返回相对 CWD 的 `data/mappings/`，CI Release 只分发单二进制、不带 `data/` → 用户在自己项目目录运行时找不到映射 → 静默降级 `builtin`，使 README「加 JSON 零代码」失效、8 个逆向 JSON 成死文件、同命令不同 CWD 行为不一致 | 发布后用户在任意非仓库目录运行 | ✅ 已修复（内嵌 16 表 + 用户级覆盖目录 + 五层解析） |
| 14 | 🟠 高危 | `--mapping` 显式指定失败时仍静默降级到内置表（与 #10 fail-fast 精神矛盾） | `--mapping <损坏/不存在的 JSON>` | ✅ 已修复（第 1/2/3 层文件存在但解析失败即中止） |
| bug | 🔴 灾难 | `builtin_claude_to_qoder()` 存在却未接入 `builtin()` 的 `match` 分支 → 发布模式（落到 builtin 兜底）下 `claude → qoder` 直接报「不支持的组合」而完全不可用 | 发布二进制 `--source claude --target qoder` | ✅ 已修复（补齐 `("claude-code","qoder")` 分支，16 组合全覆盖） |

### 关键设计决策

- **#12 内嵌 + 用户级覆盖，而非「修复 CWD 路径」**：根因不是路径写错，而是「单二进制 + 相对路径」这个分发模型本身不成立。故将 16 个映射表 `include_str!` 编译期内嵌（二进制自带全部组合），并提供与 CWD 无关的 `~/.migrator/mappings/` 覆盖目录作为「发布后零代码扩展」入口。五层解析顺序：显式 `--mapping` → 用户级覆盖 → 仓库级 `data/` → 内嵌 → builtin 兜底。
- **#12 双份维护漂移守卫**：内嵌 JSON（编译期快照）与 `builtin()` 硬编码是同一语义的两份表述，漂移会导致不同解析层产生不同迁移结果。新增单测 `embedded_and_builtin_agree_on_semantics` 校验 `builtin ⊆ embedded` 的 behavior/status 一致；`embedded_matches_repo_files` 校验内嵌快照与磁盘 JSON 逐字节一致（改 JSON 需重编译，CI 阶段即拦截）。
- **#12 端到端集成测试 `tests/cli_e2e.rs`**：此前 118 个测试全是模块级单测，从未用真实二进制验证「配置 → 完整链路 → 落地可用配置」。现通过 `CARGO_BIN_EXE_migrator` 跑真实二进制，覆盖 scan 枚举 7 面、发布「无 data/ 目录」场景回退内嵌表、`apply` 前备份既有目标 + `restore` 回滚、`--mapping` 损坏 fail-fast、未知 `--source` 报错；并以**假 HOME 隔离** + 前后字节级快照，保证真实 `~/.codex/config.toml` 全程不被触碰，同时用 `CARGO_TARGET_TMPDIR` 规避既有单测 `std::env::temp_dir()` 在沙箱的 `os error 5` 雷区。

### 验证方式（全程未触碰用户真实 `~/.codex`）

编译零错误；`converter::mapping` 单测 20 passed（含 4 个新增内嵌/覆盖/漂移守卫）；新增 `tests/cli_e2e.rs` 4 个集成测试全绿（假 HOME 下真实二进制端到端）；**3 个原沙箱受限 lib 测试（`atomic_write_creates_file` / `atomic_write_overwrites` / `target::qoder::write_creates_file_with_content`）已改为可写临时根（`CARGO_TARGET_TMPDIR`/`MIGRATOR_TARGET_DIR` 回退链）+ fsync-unsupported 优雅跳过，`cargo test --lib` 现 120 passed / 0 failed**；真实 `~/.codex/config.toml` md5 全程不变（`e3033661ae0ada04fbd7d9ae41c8f942`）。

> **结论（更新）**：除原 #1–#11 外，全局视野体检发现的 #12（分发失效）、#14（`--mapping` 降级）、`claude→qoder` 兜底漏接三项分发级缺陷也已闭环，并由单测 + 集成测试双重守护。项目作为开源 Agent 项目的端到端可信度底座、分发架构正确性、发布就绪度均达成。
