# Round 12 — Git 改动视图、Worktree 切换与本轮改动文件

> 执行方：**Windows** · 状态：✅ 已完成

## 目标

在不进入 R13 会话分支树范围的前提下，为当前项目 cwd 提供安全、可恢复的 Git status / 单文件 unified diff 查看能力、仓库顶层 worktree 的列出/创建/切换/移除，以及按 assistant turn 汇总成功 `write` / `edit` 工具调用产生的文件清单。

## 前置

- R0–R11 已完成并合并，基于 `main` 的最新结果。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- 新 worktree 内独立准备 `vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/`，并通过 `scripts/check-pins.ps1`。
- 功能行为以钉死的 `vendor/upstream/pi-web-0.8.9/` 为对照；UI 必须遵守 `docs/UI设计规范.md`。

## 交付物

- `crates/pi-data/src/git.rs`（或按现有分层拆分的等价文件）及测试：
  - `git status --porcelain=v1 -z` 解析、状态分类、当前 cwd 范围过滤、增删行统计；
  - 单文件 unified diff，覆盖 tracked / staged / untracked / renamed / deleted、二进制与超限降级；
  - worktree 列出、当前 checkout 识别、分支校验、创建与移除；
  - 移除 worktree 前检查目录 reparse point / symlink，发现目录链接必须拒绝，禁止沿链接删除共享内容；
  - Git 命令错误保持可观测，路径比较遵守 Windows 大小写与分隔符语义。
- `crates/pi-render/src/` 及测试：
  - 从单个 turn 中成功完成且未报错的 `write` / `edit`（含常见 MCP 命名变体）工具调用提取文件；
  - 相对路径按会话 cwd 解析、稳定去重、正文中仅提及的路径不得误报；
  - 文件清单附着于该 turn 的最终 assistant answer，活跃未完成 turn 不虚报。
- `crates/ui/src/`：
  - Git 状态码/统计、改动文件列表与 unified diff 原生视图；
  - `TurnWrittenFiles` 弱边框 chip，点击根内文件可打开对应文件；
  - 颜色、字号、间距、按钮、tooltip 全部遵守 `docs/UI设计规范.md`。
- `crates/app/src/`：
  - 文件面板加载 Git 状态，改动文件可打开 diff，source / diff 状态相互隔离；
  - worktree 切换器及创建/移除确认；切换后文件面板与工作区跟随新 cwd，已打开聊天仍跟随原会话；
  - cwd / worktree / 文件 tab 异步 generation 防陈旧结果覆盖；
  - `TurnWrittenFiles` 与现有文件 tab 打通，根外文件只展示、不越权打开。
- `rounds/round-12/round-12.md`、`ROUNDS.md`：完成后回填本轮实测、视觉审查与本地验收状态。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 静态与构建 | `.\scripts\validate.ps1` 全绿（pins / fmt / clippy `-D warnings` / workspace tests / release build） |
| T2 | Git status | 临时仓库覆盖 staged / unstaged / untracked / renamed / deleted / conflict 分类，Unicode 与空格路径、子目录 cwd 过滤、增删统计；非 Git 目录返回可用空态 |
| T2 | 单文件 diff | 覆盖 tracked / staged / untracked / renamed / deleted；二进制、超限、无改动与越过仓库根路径安全降级；diff 可被 `pi-render` 解析 |
| T2 | Worktree | 临时仓库覆盖 main + linked 列出、当前 checkout 识别、已有/新分支创建、重复目录/非法分支拒绝、dirty 拒绝与显式 force；main 不可移除；发现目录 symlink/junction/reparse point 时 force 也必须拒绝 |
| T2 | TurnWrittenFiles | 覆盖成功/失败/未完成工具调用、`file_path` / `path`、MCP 命名变体、相对路径、Windows 路径、去重、仅正文提及不误报；文件清单只挂最终 answer |
| T2 | UI 与状态同步 | `#[gpui::test]`/纯逻辑测试覆盖 Git 改动列表、source/diff tab、worktree 切换后 cwd 联动、陈旧 generation 拒绝、根外 written file 不可打开 |
| T2 | 用户路径烟测 | 在临时 Git 仓库完成“制造改动 → 查看 status/diff → 创建并切换 worktree → 查看 turn 文件清单 → 安全移除 worktree”，不修改真实 `~/.pi` |
| T3 | 目视 | 本地启动应用，确认深/浅主题下 Git 改动区、diff、worktree 菜单/对话框、TurnWrittenFiles 与错误/空态符合 UI 规范 |

## 禁止

- 不修改 `Cargo.lock`、`PINNED_PI_VERSION`、钉死 vendor 基线或上游版本；不执行 `cargo update`。
- 不实现 R13 的会话分支树、compaction、auto-retry 或 HTML 导出。
- 不实现 Git stage/unstage、commit、push、pull、checkout 当前 worktree 分支或冲突解决；R12 只读 status/diff，worktree 只做明确的列出/创建/切换/移除。
- 不通过 shell 拼接命令字符串；Git 参数必须逐项传给 `std::process::Command`，路径使用 `std::path::Path`。
- 不在未确认时 force 移除 dirty worktree；不移除 main worktree；发现目录 reparse point / symlink 时禁止调用 `git worktree remove`。
- 不允许 Git / written-file 路径绕过当前项目根读取任意文件。
- 不引入 WebView/HTML/浏览器技术栈，不把 diff 放进嵌套虚拟滚动表格。
- 不写入真实 `~/.pi`，测试只使用临时目录。
- 不顺手修复前序轮次问题；发现后只写入 `rounds/BACKLOG.md`。
- 不创建 PR、不推送、不合并远端。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-12/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 视觉审查

- 视觉审查模式：SCREENSHOT
- 视觉审查结论：PASS
- 截图验证：已提供（9 张最终 release 真实渲染截图）
- 兜底原因：N/A
- `requested_at`：`2026-08-20T08:18:31.1873783+08:00`
- `deadline`：`2026-08-20T08:48:31.1873783+08:00`
- 完整回传时间：`2026-08-20T08:27:29.582+08:00`
- 审查报告：`.pi/subagents/r12/visual-screenshot-final-2.md`
- 截图 manifest：`.pi/visual-review/round-12/evidence/manifest-ab8199f2f0972c5f.json`（`expectedImageCount=9`、`actualImageCount=9`、唯一 entry `2185b073`）
- 缩放元数据：`.pi/visual-review/round-12/evidence/metadata-ab8199f2f0972c5f.json`（Windows per-window DPI `168` / 显示缩放 `175%` / 应用无独立 zoom）
- 截图构建：`.pi/target-visual-validation-3/release/gpui-pi.exe`，SHA-256 `b2b1d067ede2fbcdc92ae1456a19870dca033a04846c57b8d2acd649b88965ba`
- 说明：九张证据覆盖浅色与深色主题、Worktree 展开/hover、普通与 dirty 强制移除 Dialog、non-Git muted 状态、TurnWrittenFiles chips、standalone diff 常规与横向滚动状态。专用 `visual-reviewer` 确认首审 findings 全部关闭，未发现当前可操作的 Round 12 视觉阻断项。

## 本轮实测

- 已实现 `pi-data` Git 纯逻辑层：porcelain v1 `-z` status、相对 `HEAD` 合并 diff、non-git/unsupported/命令错误区分、worktree 列出/创建/dirty+force 移除；Git 命令固定 `LC_ALL=C`，10 秒 timeout，并限制输出大小。status 的 all 输出超限时会真实回退 `--untracked-files=normal` 并标记截断，测试用小输出上限在临时仓库触发该控制流；主 worktree 通过 git common-dir 对应仓库根识别，不依赖 porcelain 输出首项或过滤后顺序。
- worktree 移除前递归检查目录 symlink/junction/reparse point；Windows junction 真测确认 force 也拒绝且外部目标文件保留。
- `pi-render` 静态与 live 文档都携带 cwd，并将 Success/Empty 且非 error 的明确 write/edit 类工具路径稳定去重后只挂到已完成 turn 的最终 assistant answer；活跃 tail 不显示。live draft 只投影 live 段并复用历史 message/item/minimap Arc。
- 文件面板新增 Git changes、错误/非 Git/无改动状态与 diff tab；status 路径对调用 cwd 相对，source/diff tab identity 与 source root 绑定，A/B worktree 同相对路径不会串 tab，重试按 tab kind 分派。
- Git changes 最多展示 500 项并显示剩余数，列表有明确高度上限和独立滚动，不会挤没文件树；untracked 行数统计有文件数/总字节预算；DiffView 持有 Arc、收集阶段即限制 2000 行并显示截断提示。
- 会话侧栏新增 checkout 顶层 worktree 切换、创建与显式移除确认；低频管理默认折叠，展开 rows 也有高度上限和滚动。独立 worktree generation 拒绝 A→B 陈旧结果，移除成功强制刷新，busy 阻止重复操作。切换只更新浏览 cwd、文件树和中心文件/diff，当前聊天仍保持原 session，且不会自动写入或绕过 project trust。
- written-file 可打开路径在投影阶段用纯词法 components 约束为 session cwd 相对路径；UI render 与 live document 不做文件系统 I/O。最终读取仍由 `ProjectFiles` canonical/reparse 门禁拒绝 symlink/junction 越根。打开请求携带 session cwd source root，绝对根外或 `..` 越界只展示并禁用，跨 worktree不会误读 browsing root 同名文件。
- 针对性验证：`cargo fmt --all`、`cargo test -p pi-data`、`cargo test -p pi-render`、`cargo test -p gpui-pi-ui`、`cargo test -p gpui-pi`、`cargo clippy --workspace --all-targets -- -D warnings` 均已通过。
- 第二轮 review 整改后完整 `validate.ps1` 实跑通过并输出 `VALIDATE OK`：app `52 passed / 1 ignored`，UI `22 passed`，pi-data unit `47 passed`，pi-render unit `19 passed`、live reducer `13 passed`，其余 workspace integration/doc tests 全绿；release build 通过。状态保持进行中。
- `Cargo.lock` 已完整恢复到 `HEAD`，`git diff --exit-code -- Cargo.lock` 退出码 `0`；`crates/ui/Cargo.toml` 无改动，UI 通过纯展示模型由 app 映射 Git 数据。
- 视觉截图首审发现 worktree 对话框 footer、折叠指示、低频删除可见性、written-file chip 弱边框及 standalone diff 长行访问问题；已完成严格限定于视图/布局/样式与对应 UI 测试的纯 UI 整改，并由最终 9 张真实渲染截图复审确认全部关闭。
- 视觉整改后使用独立 `CARGO_TARGET_DIR=.pi/target-visual-validation` 完整实跑 `scripts/validate.ps1` 并输出 `VALIDATE OK`：app `56 passed / 1 ignored`、UI `23 passed`、pi-data unit `47 passed`、pi-render unit `19 passed`、live reducer `13 passed`，其余 workspace integration/doc tests全绿；独立 target release build 通过。
- 最终代码审查剩余 UI 整改已补齐：worktree 跨行 hover 不再被旧行 leave 清空；non-repository 提示降为 muted 中文能力提示，其余错误保留 warning 原文；standalone diff 测试直接断言 gpui-component `scrollbar-overlay`，并用双轴超限 fixture 覆盖；Git 状态码统一为 `M/A/D/R/?/U`，tooltip 说明 `? 未跟踪 / U 冲突`。
- 最终独立 `CARGO_TARGET_DIR=.pi/target-visual-validation-3` validation 输出 `VALIDATE OK` 且原始退出码 `0`：app `57 passed / 1 ignored`、UI `24 passed`、pi-data unit `47 passed`、pi-render unit `19 passed`、live reducer `13 passed`，其余 workspace integration/doc tests 全绿；release build 通过。
- 最终独立代码复核结论为 `approve`，无 blocking/high/medium finding；最终 `SCREENSHOT` 视觉审查结论为 `PASS`。
- 任务卡与 `ROUNDS.md` 收口后，使用独立 `CARGO_TARGET_DIR=.pi/target-round-12-closeout` 再次完整运行 `scripts/validate.ps1`：`VALIDATE OK`、`PIPELINE_EXIT_CODE=0`，日志为 `.pi/validation/round-12-closeout-final-validate.log`。Round 12 本地验收完成，未 commit、push、创建 PR 或合并远端。
