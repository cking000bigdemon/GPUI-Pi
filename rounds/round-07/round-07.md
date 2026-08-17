# Round 07 — 活会话流式

> 执行方：**Windows** · 状态：✅ 已完成

## 目标

选择一个历史会话后可启动该会话的官方 `pi --mode rpc` 进程，在原生聊天窗口内发送基础文本 prompt，按 `message_update` 增量展示 assistant 的 text/thinking/tool-call 与工具执行状态；支持 `abort`、steer、follow-up 队列，并在用户停留尾部时平滑跟随、上滚后不抢夺视口。

## 前置

- R0–R6 已完成并合并；本轮从 `main` 的 PR #10 合并结果开始。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- `vendor/pi/pi.exe` 为钉死的官方 pi `0.84.2`；`vendor/upstream/pi-0.84.2/` 与 `vendor/upstream/pi-web-0.8.9/` 已通过 manifest 校验。
- R2 已提供 `Client`、严格 JSONL、请求关联、事件订阅和进程树终止；R6 已提供静态 `ConversationDocument` 与原生聊天组件。
- 真实 token 验收必须显式 opt-in，使用调用方已经配置的便宜模型；默认 validation 不烧 token。

## 交付物

- `crates/pi-rpc/src/`：补齐 R7 所需流式事件保真与可恢复订阅语义；事件消费不能因慢订阅者静默永久停止。
- `crates/pi-rpc/tests/`：确定性 fake-child 流式/abort/queue 测试，以及显式 ignored 的真实便宜模型对话测试。
- `crates/pi-render/src/`：不依赖 GPUI 的活会话 reducer；按 `contentIndex` 装配 text/thinking/tool-call，`message_end` 作为权威快照，维护工具状态、队列和 settled 状态，并可输出现有 `ConversationDocument`。
- `crates/pi-render/tests/`：正常文本、多 block、工具、多 turn、abort、queue、重复/乱序安全降级与 burst 合并测试。
- `crates/ui/src/chat.rs`：持久 `ScrollHandle`、尾部跟随与恢复入口；流式时不为每次重绘重建滚动身份。
- `crates/app/src/`：活会话 controller、RPC 事件泵、基础文本输入与 `Prompt`/`Abort`/steer/follow-up intent；所有 blocking RPC 命令在后台执行，session generation 防止旧事件覆盖新会话。
- `Cargo.toml` / crate manifests：只接入 R7 所需内部依赖，不改变上游钉版本。
- 本任务卡：完成后回填命令、事件/测试数字、真实模型与 T3 结果或待人工项。

## 已定语义

- **会话生命周期**：每个选中的活会话最多一个 pi 子进程；切换会话或窗口退出必须 shutdown。`agent_end` 仅结束一次低层 run，只有 `agent_settled` 才表示该轮彻底 idle。
- **增量消息**：`message_start` 初始化 assistant 草稿；text/thinking/tool-call 按 `contentIndex` 更新；`*_end.content` 覆盖累计 delta；`message_end.message` 是最终权威快照。未知或缺失事件安全降级并允许最终重同步。
- **工具状态**：`tool_execution_update.partialResult` 是累计结果，UI 应替换而非追加；工具卡从 pending 到 success/error，最终静态 session 重载可校准。
- **队列**：streaming 中通过 `Prompt.streamingBehavior` 原子提交 steer/follow-up，避免 idle 边界把直接 queue 命令搁置；`queue_update` 是完整权威快照，成功 response 只表示已接受。
- **Abort**：点击后进入 stopping，但不立即清空草稿或队列；继续消费尾部事件，直到最终 settled。失败则恢复原 phase 并显示错误。
- **滚动**：每批/每帧最多一次 follow 请求；仅在用户仍附着尾部时滚到底。用户上滚后保持位置，点击“跟随最新”或重新到达尾部后恢复。
- **性能**：事件泵必须快速 drain 并合并 delta；不允许每个 token 重读整份 JSONL。历史文档与当前 streaming bubble 分离，完成后再与持久文件校准。
- **输入范围**：R7 只提供完成真实对话所需的基础多行文本输入和流中模式选择；R8 的 IME 完整验收、附件、`@文件`、slash 面板与草稿保存不在本轮。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 格式、clippy、单测、release 构建 | `./scripts/validate.sh` 与 `./scripts/validate.ps1` 均输出 `VALIDATE OK` |
| T1 | reducer / fake RPC | `cargo test -p pi-rpc -p pi-render -- --nocapture`；覆盖 text/thinking/tool-call、工具进度、queue、abort、agent_end≠settled、>1024 burst 不静默丢终止状态 |
| T1 | GPUI | `cargo test -p gpui-pi live -- --nocapture`（或等价 focused tests）；历史/活会话、running/stopping/idle/error、队列、持久滚动 handle、上滚脱离与恢复、800×560 不溢出 |
| T2 | 官方 pi 零 token回归 | `PI_RPC_TEST_BINARY="$PWD/vendor/pi/pi.exe" cargo test -p pi-rpc --test real_pi -- --ignored --nocapture`；既有命令矩阵与 restart/resume 继续通过 |
| T2 | 真实便宜模型 | 显式设置 R7 live-test 模型/凭据后运行 ignored test；至少一轮真实文本流式对话，观测 text delta、message end、agent settled，最终 session 文件与 reducer 文本一致；另测长回复中 steer、follow-up 与 abort 的接受/queue/settled 语义 |
| T3 | 长回复流畅度 | Windows 真机复测：长 Markdown 回复持续流式时可交互、滚动无明显卡死；附着尾部自动跟随，上滚后不跳，恢复跟随后继续；记录实际模型、回复长度/事件数和体感/帧率证据 |

## 禁止

- 不实现 R8 的图片粘贴/拖拽附件、`@文件`、slash 面板、草稿持久化或完整输入框产品化。
- 不实现 R9 模型/思考级别/工具预设 UI；真实测试所需模型只通过环境或 pi 现有配置选择。
- 不实现 R10 文件浏览器、R11 git/worktree 视图、R12 分支/compaction/retry/export UI。
- 不实现 RPC 缺失的 `clear_queue`；队列以 pi 的 `queue_update` 为准。
- 不引入 WebView、HTML UI、Node/npm 运行时或网络前端资源。
- 不修改 `Cargo.lock` 中已钉的 gpui/gpui-component 身份，不执行 `cargo update`，不修改 `vendor/upstream`。
- 不破坏性写真实 `~/.pi/agent`；正常 pi 会话追加由官方子进程负责，测试一律使用隔离的临时 `PI_CODING_AGENT_DIR` / session dir。
- 不顺手修复非 R7 BACKLOG；发现跨轮次问题只登记。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-07/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- `./scripts/validate.sh`：最终 exit 0，`VALIDATE OK`；pins、fmt、workspace clippy `-D warnings`、全量 tests 与 release build 全绿。最终日志：`.pi/round-07-validate-bash-ultimate.log`。
- Windows PowerShell 5.1：仓库原 `validate.ps1` 无 BOM，直接执行会按本地代码页误读中文并 parser error；未跨轮修改上轮脚本。生成 gitignored 的 UTF-8 BOM 临时副本后全量 validation exit 0、`VALIDATE OK`；原脚本同样用 BOM 临时副本通过 `[Parser]::ParseFile`。最终日志：`.pi/round-07-validate-powershell-ultimate.log`。
- `cargo test -p pi-render --test live_reducer -- --nocapture`：**9 tests passed**，覆盖多 block 装配、`message_end` 权威覆盖、同 run 多条 user 不互相覆盖、乐观 Running 后 run identity 仍递增、历史 `Arc` 缓存、tool 累计结果替换、queue 快照替换、abort/settled 与 2048 delta burst。
- workspace 最终默认测试：`gpui-pi` **16 passed**、`gpui-pi-ui` **4 passed**、`pi-rpc` client **9 passed**；fake child 流式用例连续发 **1500 个 `message_update`** 后仍收到 `message_end` / `agent_end` / `agent_settled`。
- 官方 pi 零 token T2：`PI_RPC_TEST_BINARY="$PWD/vendor/pi/pi.exe" cargo test -p pi-rpc --test real_pi -- --ignored --nocapture` exit 0，**5 passed**；覆盖命令矩阵、首次 spawn 直接恢复指定 session、kill/restart/resume，两个 token 测试因未显式 opt-in 而诚实 skip。测试目录已改为 `tempfile` 自动清理，不在 crate 内留下嵌套 `target/`。日志：`.pi/round-07-real-pi-zero-token-post-review.log`。
- 真实 token T2 已编写两个显式 opt-in 测试：文本流式会校验 text delta、权威 `message_end`、`GetMessages`、session 文件文本与 `agent_settled`；队列测试校验 steer/follow-up `queue_update`、abort 与 settled。实际尝试隔离 `PI_CODING_AGENT_DIR` 时因隔离目录无 API key 失败（`No API key found for the selected model`），没有把共享认证复制进测试目录，也没有伪称通过。日志：`.pi/round-07-real-live-text.log`。
- 事件性能路径：RPC 事件泵以 **20ms / 最多 512 events** 合批；已定稿历史和消息改为 `Arc` 共享，流式帧只重建当前草稿与共享指针；settled 后后台重读 session 文件校准，且 activity generation 防旧校准覆盖新 run。
- 滚动路径：`ScrollHandle` 由 `ChatPanel` 持久持有；wheel 上滚、minimap 跳转以及 render 时检测到的非尾部 offset 都会解除跟随，点击“跟随最新”恢复。800×560 workspace 与 composer 不溢出测试通过。
- 独立审查按约定显式使用 DeepSeek provider 的 `deepseek/deepseek-v4-pro`。首轮发现首次 spawn 未带 session、abort 竞态、每 batch 深拷贝/重解析、user 去重与滚动缺口；均修复并补回归。终审新增的 run identity、同 run 多 user、旧校准覆盖新 run 和滚动条/PageUp 脱离风险也已由父会话继续修复并重跑全量 validation。
- T3 长回复真机目视/帧率未由自动化环境完成，需在 PR 审阅时人工启动应用，用已登录 provider 复测长 Markdown、上滚脱离与恢复；这是本轮唯一保留的人工验收项。
- 范围控制：R7 仅加入完成真实对话所需的基础 `Textarea`、发送/停止、Steer/Follow-up 模式；未实现 R8 附件、`@文件`、slash 面板、草稿持久化或完整 IME 二次验收；未实现 R9 模型/思考/工具预设。
