# Round 05 — 会话列表

> 执行方：**Windows** · 状态：✅ 已完成

## 目标

让正式 `gpui-pi` 在不启动历史会话进程的前提下，从共享 `~/.pi/agent` 后台加载真实会话，按项目归并并展示运行状态、上下文 token 与累计成本；用户可显式重命名、确认删除、导出原始 JSONL，并在选中含项目资源但尚未信任的目录时通过原生确认框写入共享 trust store。

“只读”指列表扫描、分组、统计和历史会话选择的默认数据流；重命名、删除、导出和信任仅在用户明确触发后执行。所有测试只操作 fixture 或临时目录，不修改真实 `~/.pi/agent`。

## 前置

- R0–R4 已完成并合并；本轮从 `main` 的 PR #7 合并结果开始。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- R3 已提供会话 JSONL 容错解析、项目 identity、linked worktree 归并、`trust.json` 保真原子读写和 24 个真实脱敏 fixture。
- R4 已提供正式 GPUI 窗口、Dock 左侧栏容器、原生路径选择、Root dialog/notification layer 与布局测试链。
- UI API 以钉死 `gpui cc053a4a` 与 `gpui-component 000114aa` 的本机源码/story 为准。

## 交付物

- `crates/pi-data/src/session.rs`：会话列表所需的累计 token / cost 与静态上下文 token 摘要。
- `crates/pi-data/src/session_view.rs`：按项目排序、父子会话树、运行状态 overlay 和稳定展示标题的纯逻辑视图模型。
- `crates/pi-data/src/session_actions.rs`：追加 `session_info` 重命名、仅叶会话删除、原始 JSONL 导出；共享目录操作具备明确错误与安全边界。
- `crates/pi-data/src/trust.rs`：项目资源检测、共享 `trust.json` 状态读取与显式原子信任写入。
- `crates/pi-data/src/lib.rs`：公开 R5 数据模型与动作 API。
- `crates/pi-data/tests/`：生产两层目录布局、统计、树、运行 overlay、动作和 trust fixture 测试。
- `crates/app/src/session_sidebar.rs`：后台加载、刷新、选中、重命名、删除确认、导出与项目 trust 状态编排。
- `crates/app/src/panels.rs` / `crates/app/src/workspace.rs`：把 R4 侧栏占位替换为真实会话面板，并将选择事件投影到工具栏与中心占位区。
- `crates/ui/src/project_trust_dialog.rs`：使用 gpui-component 原生 dialog API 的项目资源信任确认组件。
- `crates/app/Cargo.toml` / `crates/ui/src/lib.rs`：接入所需内部依赖与公开组件。
- 本任务卡：完成后回填实际命令、数字、限制和偏离。

## 已定语义

- **运行状态**：数据模型接受运行 ID overlay；R5 尚无活会话 registry，因此正式应用中的历史磁盘会话默认空闲，R7 接入进程后复用同一 overlay，不用改磁盘 summary。
- **上下文用量**：历史会话显示当前静态分支可确定的最近 assistant `usage.totalTokens`；没有可信 `contextWindow` 时只显示 token 数，不伪造百分比。活会话的精确 context usage 留给 R7 的 RPC stats 覆盖。
- **成本**：累计 assistant、tool result、compaction 与 branch summary 中可识别的 `usage.cost.total`。
- **导出**：R5 导出原始 `.jsonl` 副本；HTML 导出仍归 R12，避免把 Node/Web exporter 引入本轮。
- **删除**：必须二次确认；运行中的会话和仍有子会话的父会话拒绝删除，避免共享目录上的多文件非事务重挂。
- **重命名**：向 JSONL 尾部追加 `session_info`，空字符串表示清除自定义名并恢复 fallback title；运行中的会话禁用。
- **Trust**：仅用户选中/打开具体项目后显示警告，不在启动时连环弹窗；显式确认后保真原子更新 `trust.json`。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 格式、clippy、单测、release 构建 | `./scripts/validate.sh` 与 `./scripts/validate.ps1` 均输出 `VALIDATE OK` |
| T1 | 列表构建 | `cargo test -p pi-data session_view -- --nocapture`；生产两层布局可扫描，项目/会话按最近活动排序，linked worktree 归并，父子树无环，运行 overlay 正确 |
| T1 | 统计 | 24 个 R3 脱敏真实 fixture 全量构建 metrics 不 panic；token/cost 覆盖 assistant、tool result、compaction、branch summary，坏字段不拖垮整份会话 |
| T1 | 文件动作 | 临时目录验证重命名只追加合法 `session_info`、父会话删除被拒、叶会话删除、Unicode 路径原始 JSONL 导出字节一致；不触碰真实 agent dir |
| T1 | Trust | 临时项目覆盖 `.pi/extensions`、`.agents/skills`、项目 settings 资源检测；未知 trust 字段保留，显式信任原子往返一致 |
| T1 | GPUI | `cargo test -p gpui-pi session_sidebar -- --nocapture`；loading/ready/error、项目组、会话 row、运行状态、token/cost、选择、刷新、rename/delete/export/trust 入口可渲染且最小窗口不溢出 |
| T2 | 真实只读扫描 | `PI_DATA_TEST_REAL_AGENT_DIR=<真实 agent dir> cargo test -p pi-data --test real_agent_readonly -- --nocapture`；扫描 ≥20 个会话、无写入、无 panic |

## 禁止

- 不实现 R6 的历史消息正文、Markdown、diff、ANSI、图片或 minimap。
- 不实现 R7 的会话进程创建、prompt、流式、abort/steer/follow-up；运行状态仅保留可注入 overlay。
- 不实现 R8 的输入框、附件、`@文件` 或 slash 面板。
- 不实现 R10/R11 的文件浏览器、worktree 切换器或 git 视图。
- 不实现 R12 的 HTML 导出、分支导航、compaction 或 retry。
- 不猜测 context window，不用文件 mtime 猜“运行中”。
- 不对真实 `~/.pi/agent` 做自动清理、迁移或测试写入。
- 不引入 WebView、HTML UI、Node/npm 运行时或新上游版本；不修改钉死依赖。
- 不顺手修复不属于 R5 的 BACKLOG 项。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-05/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- `cargo fmt --all`：exit 0。
- focused tests：trust integration 3/3（另含 lock unit 覆盖）、session actions 2/2、session view integration 1/1、linked worktree branch 1/1、project cwd cache 1/1、sidebar 2/2、异步 trust prompt 1/1、workspace 5/5 全绿；`cargo clippy --workspace --all-targets -- -D warnings` exit 0。
- `./scripts/validate.sh`：review 修复后 exit 0，`VALIDATE OK`；全工作区 fmt、clippy `-D warnings`、tests 与 release build 全绿。日志：`.pi/round-05-review-fixes-validate-bash.log`。
- Windows PowerShell 5.1：直接只给 `validate.ps1` 加 BOM 时，子脚本 `check-pins.ps1` 仍因无 BOM 中文被误读并 parser error；这是编码误读，不计代码 validation 失败。随后在 `scripts/` 同时生成临时 UTF-8 BOM validation/check-pins 副本，validation 副本仅把调用目标替换为临时 check-pins 副本，实跑 exit 0、`VALIDATE OK`，finally 删除两个临时文件；原脚本未修改。review 修复后复跑日志：`.pi/round-05-review-fixes-validate-powershell.log`（Git Bash 捕获 PowerShell 中文仍有 mojibake，但步骤与 exit code 可判定）。
- 真实共享数据只读扫描：`PI_DATA_TEST_REAL_AGENT_DIR="$HOME/.pi/agent" cargo test -p pi-data --test real_agent_readonly -- --nocapture` review 修复后 exit 0，`sessions=174, diagnostics=0`，耗时 1.55s；日志：`.pi/round-05-review-fixes-real-agent-readonly.log`。
- DSV4 Pro finding disposition：M1 已修（扫描 revision 使用 len + modified + 稳定字节 fingerprint，rename/delete 写前复验，rename 临时文件构造后、replace 前再次复验，冲突返回 `ConcurrentModification` 并提示刷新；覆盖并发 append 拒绝测试）；M2 已修（refresh 的扫描/分组/git、rename/export/delete 文件 IO 全在 background executor，action busy 防重复点击）；L1 已修（缺少 `totalTokens` 时按存在且合法的 input/output/cacheRead/cacheWrite 分别求和）；L2 已修（保留 diagnostics 计数并在 sidebar 展示及通知）；N4 已修（GPUI test 显式断言 rename/export/delete 按钮与 diagnostics 展示存在）；L3 接受原生保存对话框的覆盖确认语义，不叠加第二次确认。终审 Note1 已修：trust 资源检测、status 读取与确认写入全部走 GPUI background executor，回主线程开/关 dialog 与通知，写入期间防重复确认，失败保留 dialog 可重试；focused GPUI test 以 executor 单步证明选择触发时主线程未直接读写。终审 Note2 已修：`list_sessions` 汇总 `load_session` 的逐行 diagnostics，header 有效的坏行会话仍显示并计入 sidebar diagnostics；生产两层布局 integration test 覆盖坏行明细。
- 终审修复后复验：`cargo fmt --all`、trust prompt focused GPUI test 1/1、生产布局 session view integration test 1/1、focused clippy 全绿；`./scripts/validate.sh` exit 0、`VALIDATE OK`（`.pi/round-05-final-notes-validate-bash.log`）；PowerShell 5.1 同目录临时 UTF-8 BOM `validate`/`check-pins` 副本全量 validation exit 0、`VALIDATE OK`，finally 删除临时副本（`.pi/round-05-final-notes-validate-powershell.log`）；真实 agent 只读扫描 `sessions=174, diagnostics=0`（`.pi/round-05-final-notes-real-agent-readonly.log`）。
- 实现限制保持不变：R5 只展示历史静态摘要，外部运行 overlay 仍待 R7；导出仅原始 JSONL，覆盖确认由原生保存对话框负责；真实 agent dir 的测试只读。`.pi` resource detection 精确保留上游 trust-manager 语义：仅检查所选 cwd 下的 `.pi`，只有 `.agents/skills` 向上查找。共享会话动作不声称完全跨进程锁定：revision 复验已显著收窄窗口，但最后一次校验与原子 replace/rename 之间仍有极小 TOCTOU 风险，终审按 accepted 处理。trust store 仅接受上游允许的 bool/null 决策；写入使用与 proper-lockfile 路径兼容的 `trust.json.lock` 目录锁和原子替换。
- 独立终审按约定使用 DeepSeek provider 的 `deepseek/deepseek-v4-pro`，最终结论：**终审通过，无 Blocker、High 或 Medium**；异步 trust 生命周期、逐行 diagnostics、pins 与轮次边界均复核通过。
