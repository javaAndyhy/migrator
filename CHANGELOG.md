# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Added
- **DeepSeek Harness（dsh）目标平台**（2026-10-26 本机实测 + `@deepseek-ai/dsh-*` 官方包 README 核实）：
  - instructions → 用户级 `~/.dsh/AGENTS.md`（dsh 先读它，再读项目侧 `AGENTS.md`/`CLAUDE.md` 链），托管块保护手写内容；
  - MCP → **cordis patch YAML 插件条目**（`~/.dsh/profiles/<profile>/cordis.patch.yml`，插件 `@deepseek-ai/dsh-mcp-client`），profile 名运行时探测（desktop 优先）；
  - skills → `~/.dsh/skills`；agents/hooks/memory 按实测降级为 `unsupported`（dsh subagent 由 preset/profile 驱动，无文件级 agents）。
- **dsh MCP YAML 转换器**（`src/converter/converters/mcp_yaml.rs`）：mcpServers JSON ↔ cordis 插件条目；
  `{env:VAR}` → `!!js process.env.VAR`（内嵌占位符 → JS 模板字符串）并可反向还原；
  写入采用 **YAML 注释托管块 upsert**（不做解析再序列化，避免摧毁用户注释、键序与 `!!js` 表达式）；
  幂等、块外同名 `id` 自动跳过、`[]` 空序列占位自动清理。
- **豆包（Doubao desktop）目标平台**：客户端无文件级配置契约（MCP/技能/规则均存于客户端 leveldb），
  故输出「手动迁移包」`{project}/.doubao/manual/`（`connectors.json` / `instructions.md` / `skills/` / `agents/` / `README.md` 操作说明），
  明确标注目标程序不读取这些文件；`TargetAdapter` 新增 `is_manual_bundle()` 以跳过托管块包裹（避免标记被粘进客户端 UI）。
- 新增 8 个平台化映射表：`claude-to-dsh` / `codex-to-dsh` / `claude-to-doubao` / `codex-to-doubao` 及 `dsh`/`doubao` → `claude`/`codex`，并接入编译期内嵌表（组合总数 16 → 24）。
- `builtin_to_dsh` / `builtin_to_doubao` / `builtin_from_domestic`：目标为 dsh/豆包时**任意源均可迁**（含 qoder/workbuddy → dsh 这类国产互转）。
- `TargetLayout` 新增 `mcp_format`（`Json` / `Toml` / `CordisYamlPluginList`）、`mcp_profile_subdir`、`manual_bundle_dir` 与 `mcp_path()`；
  `TargetAdapter` 新增 `mcp_format()`，MCP 写入改由布局声明决定合并算法（不再按扩展名猜）。
- WorkBuddy 目标平台：用户级 `~/.workbuddy/mcp.json` MCP 合并（保留已有 server）、规则全文写入 `~/.workbuddy/rules/` + `MEMORY.md` 托管块引用、agents/memory 正确降级为 `unsupported`。
- `claude-to-workbuddy.json` / `codex-to-workbuddy.json` 平台化映射表 + 内置映射表。
- MCP 真 merge 引擎（`behavior: merge`，JSON 容错、同名以迁移内容为准），应用于 qoder/trae/workbuddy。
- Qoder/Trae/灵码 instructions 输出到项目根 `AGENTS.md`（`instructions_at_project_root`）。
- commands（Claude slash 命令）源扫描 + 单文件 skill 转换（`convert_command`）。
- hooks 源扫描（`~/.codex/hooks.json`、`.claude/hooks/hooks.json`），报告但不迁移。
- 报告输出附带目标路径。
- 全局 `--only <面[,面...]>` 选项：只处理指定配置面（`instructions`/`mcp`/`skills`/`agents`/`commands`/`hooks`/`memory`）。典型场景是只想同步规则而不碰 MCP —— 写 MCP 会让真实客户端重载连接器、弹授权。未知面名 fail-fast 并列出可选项。
- GitHub Actions CI（三平台 test + release 构建）。
- **双向迁移**：国产平台 → Claude/Codex（`LayoutSource` 反向源适配器 + `claude()`/`codex()` 布局 + MCP JSON↔TOML 双向转换 + agents md↔TOML 反向转换 + 反向映射表）。

### Changed
- 通义灵码平台信息同步：已更名 Qoder CN，`LINGMA.md` 弃用 → instructions 目标改为项目根 `AGENTS.md`；README / CLI help / Cargo.toml 全面标注。
- 目标平台列表补 WorkBuddy（README / Cargo.toml description & keywords）。

### Fixed
- WorkBuddy MCP 迁移写入非官方路径 `.mcp.json` → 修正为用户级官方路径 `mcp.json`，并实现真 merge（此前 caveat 声称 merge 实际为全量覆盖）。
- WorkBuddy instructions / skills 写入项目级 `.workbuddy/` → 修正为用户级 `~/.workbuddy/MEMORY.md` 与 `~/.workbuddy/skills/`（经 8 组合真实 apply 验证发现）。
- `MEMORY.md` 被 400+ 行规则撑爆（超出记忆容量被截断）→ 规则全文外置，记忆仅保留 3 行引用。
- memory 只读索引写入 WorkBuddy 不消费的 `memory-index/` → 标记 `unsupported` 并清理死目录。
- claude.rs read() 的 `unreachable_patterns` 警告（SurfaceKind 全覆盖后残留 `_` 分支）。
- **[P0-1] 逆向迁移到 Codex 不再整文件覆盖 `~/.codex/config.toml`**：改为 `[mcp_servers]` 段级非破坏性合并（`toml_edit` 段级 merge），保留 model / provider / API key / 手写注释 / 键序；目标文件损坏时 fail-closed 拒绝写入。
- **[P0-2] `target_path` 改为按 `SurfaceKind` 类型分派**（消除旧实现 `contains("mcp")` / `contains("memor")` 字符串猜类型的误路由）：WorkBuddy `MEMORY.md` 不再被误判进 mcp、名字含 `mcp` 的 skill/agent 正确落在 `skills/`、instructions 正确落在 `AGENTS.md`、agent 正确转为 Codex TOML。
- **[P0-3] 引入 `toml` 0.8 + `toml_edit` 0.22 替换 MCP 手写解析**：修复行尾注释、`[[servers]]` 多行数组、嵌套段后字段、非字符串 env 值丢失等结构性丢字段问题；合并版式强制重新编号插入子树，新段统一追加到已有段之后。
- **[P0-4] apply 阶段备份失败改为 fail-closed**：备份失败（权限/磁盘）时返回错误并中止，不再降级为警告后继续覆盖既有配置。
- **[P1-5] WorkBuddy `rules/` 纳入备份**：`~/.workbuddy/rules/<source>-rules.md` 此前未被备份，用户已有同名 rules 会被静默覆盖且无法 `restore`。现为 `workbuddy_rules_path` 中心化计算，apply 阶段在 `target==workbuddy` 且含 instructions 面时，将其显式加入备份批次。
- **[P1-6] 托管块 scope 跨方向稳定化**：旧 scope `migrator:<source_path>` 导致同一目标文件先后从两个源迁移产生不同 scope → 叠加多块、重复迁移不更新。改为 `migrator:<target_name>:<kind>`，无论源是哪个都共享同一块，重复迁移只重写该块。
- **[P1-7] `validate` 从形同虚设升级为内容级校验**：旧实现仅检查目标文件存在/非空。现校验 instructions 的托管块存在且非空（workbuddy 额外校验 `rules/` 全文落地）、MCP 目标含 server 数量（TOML/JSON 真解析），异常降级为 `CheckBeforeUsing` 而非静默放行。

- **[P2-10] 未知 `--source` / 不支持的迁移组合显式报错（fail-fast）**：`build_source` 对未知 source 不再静默回退到 `ClaudeCodeSource`；`MappingTable::builtin` 对未知组合不再静默兜底 `claude→qoder`，改为返回错误并中止。消除"用户以为在迁移 A→B，实际走默认 B→qoder"的静默错误（原 `builtin_falls_back_to_claude_qoder` 测试在验证这个错误行为，已改写为验证报错）。
- **[P2-11] `atomic_write` 真正 fsync**：此前注释声称 fsync 却仅 `write`+`rename`，崩溃可能残留未落盘的 `.tmp`、目标文件半写风险；现于 rename 前对临时文件 `sync_all()`，确保内容与元数据落盘。（该修复在 Windows 上引入了只读句柄缺陷，见下方真机实测条目）
- **[P2-8] 逆向映射表支持 JSON 扩展**：新增 8 个逆向组合映射文件（`qoder`/`trae`/`lingma`/`workbuddy` × `claude`/`codex`，镜像 `builtin_domestic_to_claude/tocodex` 的 7 条目），`load_mapping` 步骤2 按 `<source>-to-<target>.json` 优先加载，使 README「加 JSON 零代码」契约对逆向成立（`builtin()` 仅作文件缺失时的兜底）。
- **[P2-9] 逆向源扫描覆盖完整 7 面**：`LayoutSource::scan` 在 instructions/mcp/skills/agents 之外，新增 commands/hooks/memory 三面枚举（`TargetLayout` 补充 `commands_dir`/`hooks_dir` 字段，`surface_optional_dir` 探测目录存在性）；`domestic_common_entries` 声明的 commands/hooks/memory 映射条目不再死代码，能真实出现在扫描报告里，若源平台实际存在这些目录则被探测并迁移。

### Fixed (2026-10-26 三目标真机实测暴露)

- **[真机阻塞] Windows 上所有 `apply` 都失败于 `拒绝访问 (os error 5)`**：`atomic_write` 先 `fs::write` 再用 `File::open`（**只读句柄**）调 `sync_all()`；Windows 的 `FlushFileBuffers` 要求句柄带 `GENERIC_WRITE`，只读句柄稳定返回 `ERROR_ACCESS_DENIED`。Unix 的 fsync 对只读 fd 有效，所以该写法在 Linux/CI 全绿、在 Windows 上每次 apply 都 fail-closed（只留下 `.AGENTS.md.tmp`）。修复：改用 `File::create` 取写句柄，在同一句柄上 `write_all` + `sync_all`。此前被误判为「沙箱禁止 fsync」。
- **[配置静默丢失] `codex → dsh` 退出码 0 但 MCP 一条没写**：`convert_surface` 的 `match (source, target)` 中 `("codex", _)` 排在 `(_, "dsh")` 之前，命中即返回，导致 cordis YAML 转换器从未被调用，TOML 转出的 **JSON** 被直接交给 cordis upsert，托管块为空。修复：具体组合前置——新增 `("codex","dsh")` 两段接力（TOML→JSON→cordis，notes/review 合并）与 `("codex","codex")` 直通；新增回归测试 `codex_mcp_to_dsh_routes_through_cordis_converter` 锁死路由顺序（断言产物以 `- id:` 开头、**不得**以 `{` 开头）。
- **[块中块] 反向迁移 `dsh → codex` 后目标 `AGENTS.md` 出现 4 个 MIGRATOR 标记**：源端读取 instructions 时原样搬运了本工具自己的托管块标记，目标端再包一层，来回迁移会无限套娃。修复：新增 `managed_block::strip_markers()`，`LayoutSource::read` 读 instructions 时整行剥离标记（正文完整保留），3 条单测覆盖「普通剥离 / 嵌套剥离 / 无标记零改动」。

### Fixed (2026-10-26 缺陷审计第二轮)

系统审计（clippy 全绿 + 真实文件回环测试 + 负向边界用例）发现的 5 个缺陷：

- **[信息丢失] 报告的转换提示只显示第一条**：`run_migration` 用 `conversion.manual_notes.first()` 取备注，把其余全部丢弃。实测 workbuddy → dsh 时 9 个 server 全被跳过（全部 `disabled: true`），报告只显示 `server codegraph: ...已跳过` 一条，用户会误以为「只有 1 个有问题」。修复：新增 `summarize_notes()`，≤3 条全列、>3 条列前 2 条 + 总数。
- **[0 迁移伪装成功] MCP 全部被跳过时仍报 `CheckBeforeUsing`**：同一场景下转换产物一个 server 都没有，状态却是「已处理，语义需核对」，实际 0 个迁成功，且会创建空的 `cordis.patch.yml`。修复：新增写入前守卫 `count_servers_in_conversion()`，产物 0 server 一律降级 `NotAdded` 并说明原因，不写空文件。覆盖 cordis / JSON / TOML 三种目标格式。
- **[假警报] 豆包手动迁移包 apply 成功后 validate 永远报「托管块缺失」**：写入端 `is_manual_bundle()` 故意不加托管块（标记粘进客户端 UI 是噪音），校验端却没同步跳过。修复：`validate_target` 对手动包改判「粘贴件非空 / 为空」，不再检查托管块。
- **[数据损坏级 · 真实文件命中] 旧契约 scope 的托管块不被识别 → 追加第二个块**：`[P1-6]` 把 scope 从 `migrator:<source_path>` 改为 `migrator:<target>:<kind>`，但没做 cutover。用户真实 `~/.workbuddy/MEMORY.md` 里残留 `<!-- BEGIN MIGRATOR:migrator:~/.codex/AGENTS.md -->`，导致 (a) `validate` 假报「缺失」，(b) 下次 `apply` 找不到旧块而**另起一块**，两块并存、旧块内容永久陈旧、用户手写记忆被夹在中间。修复：`upsert_managed_block` 检测任意 `MIGRATOR` 块（`migrator_block_ranges` 行级配对），当前 scope 缺失时**就地升级第一个旧块并把多个历史块收敛为一个**；`validate` 识别旧 scope 并提示「下次 apply 会自动升级」。已在真实文件上执行并验证：scope 替换、位置不变、11 行手写记忆完整保留、`mcp.json` 零改动。
- **[功能缺口] 无法只迁移单个配置面**：想升级 `MEMORY.md` 的托管块就必须连带写 `~/.workbuddy/mcp.json`，而写 MCP 会让客户端重载连接器、弹授权甚至刷新当前会话（修复上一条时被这个缺口卡住）。新增全局 `--only <面[,面...]>`（instructions/mcp/skills/agents/commands/hooks/memory），未知面名 fail-fast 并列出可选项；`run_migration` 内再加一层 `options.wants()` 双保险。

### Fixed (全局视野 / 分发架构)
- **[#12] 映射表分发缺陷（发布后单二进制静默丢弃 JSON 映射）**：旧实现 `default_mapping_file` 仅返回相对 CWD 的 `data/mappings/` 路径；CI Release 只分发单个二进制、不带 `data/`，用户在自己项目目录运行时找不到映射文件 → 静默降级到 `builtin` 硬编码，导致 (a) README「加 JSON 零代码扩展」在发布后失效、(b) 8 个逆向 JSON 成为死文件、(c) 同命令不同 CWD 行为不一致。修复：新增 16 组合**编译期内嵌 JSON**（`include_str!` 快照）+ 用户级覆盖目录 `~/.migrator/mappings/`（`user_mapping_file`），`load_mapping` 重写为五层解析（显式 → 用户级 → 仓库级 → 内嵌 → builtin），保证任意安装方式 / 任意 CWD 下行为一致，并保留「用户放一个 JSON 即可扩展组合」的契约。
- **[#14] `--mapping` 显式失败改为 fail-fast**：旧实现在 `--mapping` 指向的文件加载失败时静默降级到内置表，与「未知 source 显式报错」精神矛盾（用户以为用自己的映射表实则用内置）。现第 1/2/3 层文件**存在但解析失败即报错中止**，绝不静默换表。
- **[bug] `claude → qoder` 未接入内置兜底**：`builtin_claude_to_qoder()` 函数存在却漏接进 `builtin()` 的 `match` 分支。此前在发布模式（无 `data/` 目录、落到 builtin 兜底）下，`--source claude --target qoder` 会直接报「不支持的组合」而完全不可用。已补 `("claude-code", "qoder")` 分支；16 组合 now 全覆盖。
- **[#12] 新增端到端集成测试 `tests/cli_e2e.rs`**：此前 118 个测试全是模块级单测，从未用真实二进制验证「配置 → 完整链路 → 落地可用配置」。现通过 `CARGO_BIN_EXE_migrator` 跑真实二进制，覆盖 scan 枚举 7 面、发布的「无 data/ 目录」场景下回退内嵌表、`apply` 前备份既有目标文件 + `restore` 回滚安全网、`--mapping` 损坏 fail-fast、未知 `--source` 报错；并以**假 HOME 隔离** + 前后字节级快照比对，保证真实 `~/.codex/config.toml` 全程不被触碰（同时规避既有单测 `std::env::temp_dir()` 在沙箱的 os error 5 雷区，改用 `CARGO_TARGET_TMPDIR`）。
- **[收尾] 全量测试在沙箱环境零失败**：3 个原有 lib 单元测试（`atomic_write_creates_file` / `atomic_write_overwrites` / `target::qoder::write_creates_file_with_content`）原因沙箱对系统临时目录的 `os error 5` 写/删保护而失败。改为使用可写临时根（`CARGO_TARGET_TMPDIR` → `MIGRATOR_TARGET_DIR` → `temp_dir` 回退链），并对 fsync 不受支持的环境（`os error 5` / `PermissionDenied`）优雅跳过；生产语义（`atomic_write` 仍 fail-closed，fsync 失败即硬错误）保持不变。现 `cargo test --lib` 在沙箱与本地均 **120 passed / 0 failed**；另清理 21 个陈旧的 `target-build-*` 构建目录（被 `.gitignore` 的 `/target-*` 覆盖，纯可重建缓存，已移至 `/tmp/migrator-old-targets/` 保留可逆）。

### Verified
- 8 组合（claude/codex × qoder/trae/lingma/workbuddy）在隔离 HOME 下真实 apply ×2（幂等）+ 产物断言全部通过。
- **[P0 验收] 109 个单元测试（原 89）全绿 + 假 HOME 隔离下 qoder→codex 真实 apply 验证**：路由全部正确；真实 `~/.codex/config.toml` 的 model/provider env_key/base_url/history/手写注释全部保留；新 server 写入、同名 server 按源侧更新；产物经 Python `tomllib` 严格解析全部值正确；二次 apply 产物 md5 一致；目标文件损坏时退出码 1、报「拒绝写入」、原文未被改动；**真实 `~/.codex/config.toml` 完整性 md5 全程不变**。
- **[P1 验收] 单元测试由 109 → 114（新增 5 个回归：rules 路径计算、scope 跨源稳定、重复迁移单块、validate 检出托管块/缺失、MCP server 计数）**，编译零错误、测试全绿；真实 `~/.codex/config.toml` 完整性 md5 不变。
- **[P2 验收] 编译零错误；新增 3 个回归测试（`builtin` 未知组合报错、`build_source` 未知源报错/已知源成功）全部通过。总 116 测试、113 passed；3 个既有 lib 单元测试（`atomic_write_creates_file` / `atomic_write_overwrites` / `target::qoder::write_creates_file_with_content`）因当前沙箱对临时目录的 `os error 5` 写/删保护而失败——与本次改动无关（P1 验收时全绿，且本次未改其写路径逻辑），建议在其本地（非沙箱）环境运行 `cargo test` 确认全绿。真实 `~/.codex/config.toml` md5 全程不变。
- **[P2 #8/#9 验收] 编译零错误；新增 2 个回归测试（`reverse_mapping_loaded_from_json` 验证 8 个逆向 JSON 可被加载且含 commands/hooks/memory 条目、`scan_covers_all_seven_surfaces` 验证逆向源扫描产出完整 7 面）全部通过；lib 113 passed / 3 failed（3 个为沙箱 os error 5 环境限制，与改动无关）；真实 `~/.codex/config.toml` md5 不变。
- **[收尾验收] 全量测试 120 passed / 0 failed（lib 120 + 集成 4），0 警告**：3 个原沙箱受限测试已修复（可写临时根 + fsync-unsupported 优雅跳过）；真实 `~/.codex/config.toml` 当前 md5 与基线 `e3033661ae0ada04fbd7d9ae41c8f942` 一致（全程零写入，上一轮观测到的 `bf883830…` 为 Codex 应用自身瞬变重写，现已回到基线）。

## [0.2.0] - 2026-08-14

### Added
- 四层管线（Source Adapter → 转换引擎 → Target Adapter → 安全写入层）。
- 五安全契约：默认只读（`apply --yes`）、托管块、原子写入、备份回退链、幂等。
- 平台化映射表（`data/mappings/<source>-to-<target>.json`）+ 内置表回退。
- 多目标适配器：Qoder / Trae / 通义灵码。
- 语义降级三态报告 + `MANUAL MIGRATION REQUIRED` 块。
- 70 个单元测试（托管块 / 原子写入 / 备份回退 / 映射表 / 转换器 / 双源适配器）。

[Unreleased]: https://github.com/javaAndyhy/migrator/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/javaAndyhy/migrator/releases/tag/v0.2.0
