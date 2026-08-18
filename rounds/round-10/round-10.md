# Round 10 — 模型、思考级别与工具预设

> 执行方：**Windows** · 状态：✅ T1/T2/T3 全部通过 · 完成日期：2026-08-18 · PR：[#17](https://github.com/cking000bigdemon/GPUI-Pi/pull/17)

## 目标

在不修改钉死 pi 0.84.2 的前提下，使活会话可在原生 UI 中切换模型与思考级别、通过快捷键循环模型，并可选择工具预设；工具集合变化时按“终止当前子进程 → 携新 `--tools` 参数重启 → resume 原会话”完成切换，且历史/活会话中的助手消息按《UI 设计规范》S-20 展示实际模型名。

## 前置

- R0–R9 已完成并合并；本轮基于 `main` 的 PR #16 合并结果。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- 新 worktree 内独立准备 `vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/`，并通过 `scripts/check-pins.ps1`。
- RPC 与 CLI 行为以钉死的 `vendor/upstream/pi-0.84.2/` 为准，功能形态以 `vendor/upstream/pi-web-0.8.9/` 为对照。

## 交付物

- `crates/pi-render/src/lib.rs`、相关测试/fixture：
  - 增加 `ModelRef { provider, id }` 与 `Message.model: Option<ModelRef>`；折叠 session 条目时跟踪 `model_change` 游标，模型变更条目不占正文。
- `crates/pi-rpc/src/`、相关测试：
  - 补齐本轮需要的模型/思考状态与命令通路；工具预设仍通过启动参数实现，不伪造上游不存在的 RPC 工具开关。
- `crates/app/src/live_session.rs`、`crates/app/src/panels.rs`、`crates/app/src/workspace.rs`（按实际分层落点）：
  - 模型选择、思考级别选择、模型循环快捷键；
  - 工具预设选择及带 `--tools` 参数重启并恢复同一会话；
  - 切换中的 busy/错误状态与重复触发防护；
  - 控件遵循 `DropdownButton`/`Popover`、主题 token、tooltip、信息片段数等 UI 规范。
- `crates/ui/src/chat.rs`：助手消息在正文上方显示模型名；模型信息缺失时不渲染空行。
- `rounds/round-10/round-10.md`、`ROUNDS.md`：完成后回填实测与本地验收状态。
- `scripts/fetch-pi-source.ps1`、`scripts/fetch-pi-web.ps1`：本轮启动门禁中经用户明确授权修复 Git Bash 抢占 `tar.exe` 的问题，固定调用 Windows 系统 `tar.exe`，并完成语法与门禁实测。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 静态与构建 | `.\scripts\validate.ps1` 全绿；若 WinPS 5.1 仍受既有 BOM 问题阻塞，则按脚本定义逐项执行 `check-pins` / fmt / clippy / test / release build，内容不得打折 |
| T2 | 模型元数据 | fixture 覆盖：`model_change` 仅更新游标；后续 assistant 消息携带正确 `ModelRef`；变更前消息不被回写；缺失模型时为 `None` |
| T2 | 真实 pi 模型/思考命令 | 使用 `vendor/pi/pi.exe --mode rpc` 的无 token 黑盒测试覆盖状态读取、合法 thinking level 设置及模型循环/切换协议；错误响应可观测且不污染客户端状态 |
| T2 | 工具预设重启 | 自动化测试证明工具参数从所选预设生成，切换保持原 session path/id、旧进程被终止、新进程携新 `--tools` 启动；失败时进入可恢复错误态，不出现双进程 |
| T2 | UI 交互与 S-20 | `#[gpui::test]` 覆盖模型/thinking/tools 选择器状态、运行中禁重入、Ctrl+P 模型循环，以及助手消息模型名的显示/缺失条件与 dim 样式数据通路 |
| T3 | 用户验收 | 本地启动应用：实际切换模型、thinking、工具预设并继续同一会话；确认模型名显示、控件观感、错误反馈与会话连续性。用户签字后才将本轮视为最终验收完成 |

## 禁止

- 不修改 `Cargo.lock`、`PINNED_PI_VERSION`、钉死的 vendor 基线或上游版本。
- 不给 pi 二进制打补丁，不伪造 `set_active_tools` 一类上游不存在的 RPC 命令；工具切换只能走带新启动参数的重启 + resume。
- 不实现 R11 文件浏览器、R12 git/worktree、R13 分支树/compaction/retry/export、R16 模型配置/API Key/OAuth。
- 不引入 web 技术栈，不新增 WebView/HTML。
- 不在组件中硬编码颜色、字体或自行计算浮层坐标；遵守 `docs/UI设计规范.md`。
- 除用户明确授权的两个 fetch 脚本修复外，不顺手修复其他历史轮次问题；发现后写入 `rounds/BACKLOG.md`。
- 不创建 PR，不推送 GitHub。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-10/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

> 执行环境：Windows 11 Pro · Windows PowerShell 5.1 · rustc 1.97.1-x86_64-pc-windows-msvc
> 工作目录：`D:\variFlight_work\GPUI-Pi-round-10` · 分支：`WinClaude/round-10`

### 启动门禁与脚本修复

- worktree 创建后，在读取任务卡/源码前独立执行四步门禁。
- 首次 `fetch-pi-source.ps1` 从 Git Bash 启动时，PATH 中的 GNU `tar.exe` 把 `D:\...` 误判为 `remote:file`，报 `Cannot connect to D`。
- 经用户明确授权，本轮修复 `scripts/fetch-pi-source.ps1` 与 `scripts/fetch-pi-web.ps1`：真正解压时按 `Sysnative\tar.exe` → `System32\tar.exe` 定位 Windows 系统 tar；已准备目录的快速校验路径不依赖 tar。
- 两个脚本均通过 PowerShell AST 语法解析；从头重跑四步门禁全绿：
  - `vendor\pi\pi.exe`：`0.84.2`；
  - pi 源码：marker 正确、无 `.git`、**1373** 个文件与 manifest 全量一致；
  - pi-web：marker 正确、无 `.git`、**380** 个文件全量一致；
  - `scripts/check-pins.ps1` 全部 OK。
- 最终递归扫描 worktree：**0 个 reparse point**。

### 实现结果

- 模型与 thinking：活会话启动后读取 `get_state`、`get_available_models`、`get_available_thinking_levels`；选择器切换后回读权威状态，thinking 能力裁剪与模型切换连带变化不会造成 UI 漂移。
- 模型循环：`Ctrl+P` 在活会话 Idle 且非 busy 时调用官方 `cycle_model`；没有活会话或运行/busy 时不吞键、不触发。
- 工具预设：`跟随 pi`（默认，不追加 `--tools`）、关闭、只读、内建四件套、完整七件套。非 Inherit 预设使用严格 CLI allowlist，文案明确扩展工具不生效。
- 工具切换：只允许 Idle；旧 Client 先 `shutdown` 并等待旧 pi 完全退出，再以同一绝对 session path、同一 cwd、所选 `--tools` 启动新 Client。重启期间启动/发送/其他控制禁入；过期 generation 的新进程会转后台 shutdown，不在 UI 线程 join，也不会遗留双进程。
- 工具预设可在启动活会话前选择，首个 pi 进程直接带目标策略启动，不需要先以宽权限启动后再重启。
- S-20：`pi-render::Message` 新增 `model: Option<ModelRef>`；静态路径只在 selected branch 上跟踪 `model_change`，assistant wire metadata 优先，字段不完整时清空不猜；活消息保留 provider/model。UI 优先显示当前模型目录中的 display name，未命中回退原始 id，缺失时不渲染模型节点。

### T1 全量验证

最终连续执行：

```powershell
.\scripts\validate.ps1
```

结果：**VALIDATE OK**。

| 步骤 | 结果 |
|---|---|
| pins | zed / gpui-component 无杂散 sha；pi 1373 文件、pi-web 380 文件全量一致 |
| `cargo fmt --all -- --check` | OK |
| `cargo clippy --workspace --all-targets -- -D warnings` | OK |
| `cargo test --workspace` | 全绿；`gpui-pi` **37 passed / 1 opt-in ignored**，`gpui-pi-ui` **18 passed**，`pi-data` 及集成测试全绿，`pi-render` 及集成测试全绿，`pi-rpc` **8 + 12 passed**；真实 pi 5 例仍按环境 opt-in ignored |
| `cargo build --release --workspace` | OK |

上游依赖 `proc-macro-error2 v2.0.1` 仍打印 future-incompat warning；不是本轮引入，且未影响 `-D warnings` clippy/构建结果。

### T2 专项验证

1. **真实 pi 0.84.2 零 token 黑盒**

```bash
PI_RPC_TEST_BINARY="D:/variFlight_work/GPUI-Pi-round-10/vendor/pi/pi.exe" \
  cargo test -p pi-rpc --test real_pi zero_token_command_matrix -- --ignored --nocapture
```

结果：**1 passed**。覆盖 typed state、模型目录、thinking levels、合法 thinking 设置、模型/thinking 循环（能力存在时）、无效模型错误、session/tree/stats/commands/bash 等无 token 命令。

2. **fake child typed 控制链**

```bash
cargo build -p pi-rpc --bin fake_child
GPUI_PI_TEST_FAKE_CHILD="D:/variFlight_work/GPUI-Pi-round-10/target/debug/fake_child.exe" \
  cargo test -p gpui-pi session_controls_and_switches_use_typed_rpc_state -- --ignored --nocapture
```

结果：**1 passed**。覆盖 controls 初始加载、模型循环、显式模型切换、thinking 切换及 canonical 回读。

3. **工具参数与恢复**

- `pi-rpc/tests/client.rs`：默认/空/只读等 `--tools` 参数传递；旧 Client shutdown 后新 Client 以同一 `--session` 路径启动，pid 变化且 allowlist 变化；空 allowlist 作为显式参数保留。
- `panels.rs`：预启动选择工具预设；当前/过期 ToolRestartFinished 错误状态；运行/busy 启用矩阵；generation 过期隔离；工具重启时校准计数器重新从 0 对齐。

4. **渲染与 UI**

- 静态模型游标：变更前不回写、变更后生效、wire metadata 优先、字段不完整清空；
- live reducer：start/delta/end 保留模型元数据；
- S-20：有模型节点则渲染 `assistant-model`，缺失则不渲染；display name 映射与 id fallback；
- composer：模型/thinking/tools 选择器渲染、Ctrl+P 判定、busy/运行态控制守卫。

### 独立代码审查

通过 `claude-code-review`（Claude Opus 5 / xhigh）完成三轮只读审查：

- 第一轮发现 1 high + 6 medium：重启窗口可重复启动导致双进程、pump 误退出、过期结果 UI 线程析构、默认 `--tools` 覆盖用户配置、测试空转等，均已修复。
- 第二轮复核上述问题关闭，补修预启动工具选择、工具重启失败测试、真实 cycle 覆盖、显示名映射、Sysnative 快速路径等。
- 第三轮未再发现进程并发、session 文件并发写、UI 线程阻塞或模型/thinking 状态不一致 blocker；唯一实质问题是重启后 calibration generation 未对齐，已在最终 validation 前修复。

### T3 用户验收结论

用户于 2026-08-18 完成验收，结论：**全部通过**。验收过程中一度误以为模型只能通过快捷键循环；确认点击「启动活会话」并完成模型目录加载后，可通过模型下拉菜单手动选择，`Ctrl+P` 仅为辅助循环快捷键，行为符合本轮设计。

已通过项目：

1. 选择一个有历史消息的会话，确认 assistant 消息顶部显示模型 display name；老消息没有模型信息时不出现空行。
2. 启动活会话，确认模型、Thinking、工具三个选择器出现且当前值正确；运行中选择器不可用。
3. 切换模型，确认按钮短暂显示「切换中…」，完成后显示新模型；`Ctrl+P` 能循环模型。
4. 切换 thinking，确认完成后显示 pi 实际采用的级别。
5. 在未启动活会话时先选「只读」，再启动；让 agent 尝试 bash/write，确认工具不可用。
6. 在 Idle 状态从「只读」切到「完整」，确认会话历史保持、可继续对话、任务管理器无残留旧 `pi.exe`；再切回「跟随 pi」。
7. 深/浅主题下确认三个选择器、下拉菜单和 assistant 模型名观感符合规范。

### 偏离与剩余风险

- 官方 RPC 0.84.2 没有查询 active tools 的命令，自动化只能验证 CLI 参数、进程时序和 session resume；实际工具可用性必须在 T3 通过只读/完整预设各跑一次确认。
- 纯历史浏览在活会话启动前没有模型目录，只能显示原始 model id；启动活会话加载 model catalog 后会优先显示 display name。
- 控制切换采用 30s RPC 超时；极端 pi 卡死时 busy 反馈可能较久，但失败后会解锁并保留错误信息。
- 用户最初要求不创建 PR、不推送；T3 通过后已明确授权创建 PR、运行 CI、CI 通过后合并到 `main` 并清理分支。PR 已创建为 [#17](https://github.com/cking000bigdemon/GPUI-Pi/pull/17)，合并后需核验 `main` 包含最终提交。
