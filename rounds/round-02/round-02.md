# Round 02 — `pi-rpc`：子进程 + JSONL 协议

> 执行方：**Windows** · 状态：✅ 已完成

## 目标

让 `pi-rpc` 在不依赖 GPUI 的前提下，完整驱动钉死的 `pi 0.84.2 --mode rpc`：严格 JSONL 分帧、请求响应关联、事件广播、进程树终止，以及异常退出后自动重启并恢复原会话。

## 前置

- R0、R1 已完成并合并；R1 四条风险门禁全绿。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- 官方 `pi 0.84.2` 二进制可用；本地 T2 可通过 `PI_RPC_TEST_BINARY` 显式指定。
- 协议权威来源：`earendil-works/pi` tag `v0.84.2` 的 `packages/coding-agent/docs/rpc.md` 与同 tag 源码。

## 交付物

- `crates/pi-rpc/src/protocol.rs`：`v0.84.2` 的完整命令、响应数据、事件与 Extension UI serde 类型。
- `crates/pi-rpc/src/jsonl.rs`：仅按 LF 切帧、剥尾部 CR、保留 `U+2028/U+2029`、支持无尾 LF 最后一帧。
- `crates/pi-rpc/src/process.rs`：子进程监督、stdin/stdout/stderr 管理、请求 id 关联、事件广播、异常退出重启与 session resume。
- `crates/pi-rpc/tests/client.rs`、`crates/pi-rpc/tests/fixtures/fake_child.rs`：并发关联、广播背压、进程树终止、异常重启与 shutdown 兜底黑盒测试。
- `crates/pi-rpc/tests/real_pi.rs`：真实 `pi 0.84.2` 的不烧 token 命令矩阵与 kill/restart/resume 黑盒测试。
- `crates/pi-rpc/src/lib.rs`、`crates/pi-rpc/Cargo.toml`：公开 API 与依赖接线。
- 本任务卡：完成后回填实际命令、数字、踩坑与设计偏离。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 格式、clippy、单测、release 构建 | `./scripts/validate.sh --logic` 与 `./scripts/validate.sh` 均输出 `VALIDATE OK` |
| T2 | 真实 pi 命令矩阵 | `PI_RPC_TEST_BINARY=<官方 pi 0.84.2> cargo test -p pi-rpc --test real_pi -- --ignored --nocapture --test-threads=1`；覆盖 `get_state`、`get_commands`、`get_messages`、`get_entries`、`get_tree`、`get_session_stats`、thinking/queue/auto-compaction/auto-retry 开关、`bash` 事件 id 关联、错误响应、新会话等不调用 LLM 的路径 |
| T2 | 断电自愈 | 同一黑盒测试中外部 kill 当前子进程；客户端在限定时间内收到退出/重启生命周期事件，后续 `get_state` 成功，`sessionId` 与会话内容保持一致 |
| T2 | 协议矩阵静态覆盖 | 单测断言所有 `v0.84.2` 命令 variant 可序列化、文档/源码事件可反序列化；严格 JSONL 边界用 chunk/CRLF/Unicode separator 覆盖 |
| T3 | 无 | 本轮为纯逻辑 crate，不需要目视验收 |

> 立项文档写的是“39 个命令/事件类型”，但钉死的 `v0.84.2` 源码实际为 **32 个 RPC command**，并另有 session/agent 事件与 Extension UI 子协议。本轮以钉死 tag 的源码为准完整覆盖，不伪造不存在的类型凑数；最终实测回填精确数量。

## 禁止

- 不接 GPUI、不改 UI crate 或 app 正式界面（R4+）。
- 不发送会消耗模型 token 的真实 prompt、compact 或分支摘要请求。
- 不实现 `~/.pi/agent` 文件层与会话文件业务解析（R3）。
- 不实现活会话消息渲染与输入队列 UI（R7/R8）。
- 不改 `Cargo.lock` 中钉死的上游 GPUI/pi 版本，不运行 `cargo update`。
- 不顺手修复 BACKLOG 中属于其他轮次的问题。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-02/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- 协议静态覆盖以钉死源码为准：32 个 command、23 个 session/agent event name、`extension_error` 与 9 种 `extension_ui_request`；开放的 provider/extension/session payload 使用 `serde_json::Value` 保真，稳定 envelope 与字段强类型。
- JSONL 单测覆盖 chunk 边界、CRLF、U+2028/U+2029、无尾 LF 和帧上限。
- fake child 黑盒覆盖并发 request id 关联、stderr drain、进程树强杀、旧 pending 失败、带最近 `get_state.sessionFile` 的自动 restart/resume，以及主动 shutdown 不重启。
- 真实 pi：`PI_RPC_TEST_BINARY=D:/variFlight_work/GPUI-Pi/vendor/pi.exe cargo test -p pi-rpc --test real_pi -- --ignored --nocapture --test-threads=1`，2 passed，精确版本 0.84.2；覆盖零 token 查询/设置/错误/bash/new session 矩阵和 switch fixture 后 kill/restart/resume。
- T2 隔离：每次真实测试都把 `PI_CODING_AGENT_DIR` 与 `--session-dir` 指到仓库 `target/pi-rpc-tests/` 临时目录，并开启 `--offline --no-extensions --no-skills --no-prompt-templates --no-context-files`；测试前后真实 `~/.pi/agent/settings.json` SHA256 均为 `7298af9b…`。
- Windows 路径踩坑：真实用户 home 含非 ASCII 时官方 binary 回报的默认 session path 可能不可直接用于 Rust 文件探测；真实 resume 测试改用仓库 `target/` 下显式 fixture 和 `switch_session`，通过 `set_resume_session` 明确交给监督器，再验证同一路径 `--session` 恢复。
- CI 跨平台修复：Linux 的 procps-ng `kill` 会把裸 `-{pid}` 误解为旧式 signal 参数，导致进程组未终止和测试等待 60 秒；Unix 调用改为 `kill -TERM -- -<pgid>`，用 `--` 明确结束选项。
- 审查后收口：在 restart delay 的下一次 spawn 前检查 shutdown，避免主动关闭期间多起一个进程；`new_session` / `switch_session` 的上层可用 `set_resume_session` 立即更新恢复目标，无需等待下一次 `get_state`；无持久会话的 `get_state` 会清除旧恢复目标；事件订阅改为 1024 条有界缓冲并断开落后的消费者；graceful shutdown 超时后自动进程树强杀，失败时按 100ms 重试；fake child 增补慢订阅者、ephemeral 状态清除与不处理 stdin EOF 三条黑盒测试。
- 最终 validation：`./scripts/validate.sh --logic` 与 `./scripts/validate.sh` 均输出 `VALIDATE OK`；`pi-rpc` 为 8 个协议/JSONL 单测 + 7 个 fake child 黑盒测试，真实 T2 为 2/2 passed。
- PowerShell 说明：本机仅有 Windows PowerShell 5.1，直接读取仓库 UTF-8 无 BOM 中文脚本会误解码并 parser error，因此本轮未能直接执行 `validate.ps1`；没有修改该脚本。等价的 `validate.sh --logic` / 全量 cargo 门禁均在本 Windows 环境通过，真实 pi T2 也通过。
- PR #5 CI：首次 `windows (阻断)` 5m44s 通过；`linux 纯逻辑 (非阻断)` 暴露 Unix `kill` 参数歧义后已修复。修复轮次 run `31942339800` 中 Linux 47s、Windows 4m39s，两个 job 均为 pass。
