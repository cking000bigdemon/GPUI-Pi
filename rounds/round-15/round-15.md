# Round 15 — 项目命令环境扩展

> 执行方：**Windows** · 状态：已完成（PR [#24](https://github.com/cking000bigdemon/GPUI-Pi/pull/24) 待审）

## 目标

在不依赖 R13/R14 的前提下，让 GPUI-Pi 活会话通过随包 TypeScript host extension 获得项目 bash 环境隔离；在 pi 0.84.2 可观察范围内避让用户 bash tool，并保证 None/ReadOnly 不抢 direct `user_bash`。

## 交付物

- `crates/pi-rpc/assets/project-command-environment.ts`
  - Windows 大小写不敏感删除 `PORT`、`NODE_ENV`、`NEXT_*`，保留其他环境、`PI_*` 和 agent `bin` PATH；命令内显式变量仍可覆盖。
  - 每个 extension 实例在首次 `session_start` 创建一次 `SettingsManager`；execute/direct bash 只调用缓存 manager 的 getter。
  - 在 `resources_discover` startup/reload handler 中复检 owner 并幂等注册，不返回额外资源。该事件位于所有 awaited `session_start` handlers 之后，异常由 extension runner 的 handler try/catch 捕获。
  - agent execute 使用本次 `executionContext.cwd` 构造 tool definition，不复用旧 ctx cwd。
  - host owner 通过 `import.meta.url` / `fileURLToPath` 与 `sourceInfo.path` 的规范化路径比较，不修改 LLM 可见 description。
- `crates/pi-rpc/src/host_extension.rs`
  - `include_str!` 嵌入，临时文件 + `sync_all` + rename。
  - 主内容寻址目标损坏时优先复用同目录中已校验一致的 fallback；没有时才创建唯一后缀。
- `crates/app/src/live_session.rs` / `panels.rs`
  - 首次启动和 tools preset restart 共用配置。
  - Rust materialize 失败时不带 `-e` 降级启动，经现有提示区域持久显示“项目命令环境扩展未加载”，未新增 UI 组件类型。该会话级状态与普通 `rpc_error` 分离，成功请求/控制不会清除；新 generation 或正常 tools restart 清除，仍降级的 restart 保留。
- `crates/pi-rpc/tests/real_pi.rs`
  - 真实钉死 pi、临时 agent/session 目录、零 token ignored T2。

## 验收

| 级别 | 检查 | 期望 |
|---|---|---|
| T1 | fmt / tests / full validation | package tests 与 `scripts/validate.ps1` 全绿 |
| T2 | 扩展落盘 | 完整稳定；损坏主目标自愈并连续复用同一 fallback |
| T2 | 启动降级 | ToolPreset 语义不变；materialize 失败仍可启动并产生诊断 |
| T2 | direct RPC 环境 | host 生效时清洗变量、保留 PATH、显式变量可覆盖 |
| T2 | owner/元数据 | owner 路径为 host，description/prompt metadata 保持 upstream 原值 |
| T2 | 优先级 | CLI override、自动发现 override、async `session_start` 注册 override 不被 host 抢占；ReadOnly user_bash-only handler 生效 |

## 禁止

不实现 R13/R14；不改 `Cargo.lock`、pins、vendor upstream 或真实 `~/.pi`；不提交、推送或建 PR。

## 视觉审查

- 视觉审查模式：SCREENSHOT
- 视觉审查结论：PASS
- 截图验证：已提供（4 张）
- `requested_at`：`2026-08-20T13:47:50.0348365+08:00`
- `deadline`：`2026-08-20T14:17:50.0348365+08:00`
- 证据清单：`.pi/visual-review/round-15/evidence/manifest-f97bddfc731a4815.json`，`actualImageCount == 4`，条目 `e7a05a72`。
- 覆盖状态：浅色/深色故障降级横幅，以及浅色/深色正常无横幅状态；窗口 `2880×1716`，Windows 显示缩放 `175%`。
- 结论：warning 在两种主题下完整可读、无裁切，仅使用语义文字色而未铺色；正常状态移除诊断行且不残留空白，超宽窗口下未见溢出或布局抖动。

## 已知限制与口径

- `SettingsManager` 按 extension 实例缓存，对齐 pi-web。设置文件变化不会在每次 bash 自动刷新，需要 runtime reload/新 extension 实例后生效；这样避免每次命令在共享 agent 目录创建/争用 lock 并发生最长约 200ms 忙等。
- CLI host 排在自动发现 extension 前，pi 0.84.2 API 不能枚举后续 `user_bash` handlers。默认工具集下，后加载且只注册 `user_bash`、不注册 bash tool 的 extension 无法可靠检测。
- `resources_discover` 可避让加载期和 awaited async `session_start` 注册；但在 `resources_discover`/host 注册之后才动态注册同名 bash 的 extension 仍可能被排首位的 CLI host 遮蔽。不得宣称所有显式/自动发现 override 都无条件优先。该偏差已记 `rounds/BACKLOG.md`。
- None/ReadOnly 下 host 不接管 direct bash。若没有用户 `user_bash` handler，direct RPC bash 回退 pi 原生 operations，**不做项目环境清洗**；这是避免抢占 user_bash-only handler 的有意取舍。
- 当前可见降级诊断只覆盖 Rust materialize 失败；pi 侧 `extension_error` 尚未映射到 GPUI 的现有 Diagnostic 通道。
- agent tool execute 仅完成注册、owner、description、prompt metadata 实测；实际 execute 环境行为由与 direct RPC 共用的 `projectCommandOperations` 路径推导，未烧 token执行。
- host 生效时按 pi-web 规则删除继承的 `PORT`/`NODE_ENV`/`NEXT_*`，相对终端 pi 有意存在差异。旧版本 temp 清理和 POSIX `/tmp` 权限留后续范围。

## 本轮实测

- host 在首次 `session_start` 仅创建一次缓存 `SettingsManager`；`resources_discover` 复检明确 built-in owner 后才注册，重复 startup/reload 不重复注册。
- owner 判断使用 extension 自身文件 URL 规范化路径；真实 pi probe 断言 source/path 为 materialized host，description 为 upstream 原始 bash description，prompt guideline 原样保留。
- async `session_start` 测试先 `await setTimeout(10)` 再注册用户 bash；`resources_discover` 时最终 owner 与 direct handler 均为用户 extension。
- ReadOnly、async 动态、自动发现三条优先级测试均附带 owner probe，断言最终 host 未成为 bash owner；host extension 本身能够加载并注册由 `project_command_environment_sanitizes_direct_rpc_bash` 的正向 owner/path/metadata 断言单独覆盖。
- `wait_for_entry` 在找到 custom entry 但缺 data 时立即 panic 并打印 entry，不再等待至超时。
- materialize 单测断言损坏主目标后连续两次返回同一已校验 fallback；fallback 扫描遇到目录形态/不可读候选时跳过，并继续复用后续有效候选。
- 真实 pi T2：环境隔离、CLI override、ReadOnly user_bash-only、async session_start override、自动发现 user scope 五条逐项显式运行，均为 `1 passed / 0 failed`，全程零 token。
- ChatPanel 测试覆盖：收到当前 generation 的 degradation Diagnostic 后，普通 `rpc_error` 清理不影响降级状态；陈旧 generation 不覆盖；开始新 generation 清除。tools restart 的状态由新 `ActiveSession::startup_diagnostic` 决定，正常进程清空，仍降级进程保留；渲染测试还双向断言 `host-extension-degradation` selector 的出现与移除。
- `cargo test -p pi-rpc`：unit `12 passed`、client `12 passed`、真实 tests 默认 `10 ignored`；`cargo test -p gpui-pi`：`59 passed / 1 ignored`。
- 完整 `scripts/validate.ps1` 输出 `VALIDATE OK`：pins、fmt、workspace clippy `-D warnings`、workspace tests、release build 全绿。主要计数：app `59 passed / 1 ignored`、UI `24 passed`、pi-data unit `47 passed`、pi-render unit `19 passed`、live reducer `13 passed`、pi-rpc unit `12 passed` + client `12 passed`。
- 独立代码终审最终结论 `approve`，无 blocking/high/medium findings；视觉审查最终结论 `SCREENSHOT / PASS`，无阻断项。
- PR 创建后合入已包含 R13 的 `origin/main`（`d3aca23`），冲突解决同时保留 R13 的 `agent_dir`、会话控制结果/状态条与 R15 的 host extension 配置、generation-scoped degradation warning；集成终审再次 `approve`。
- 合入 R13 后完整 `scripts/validate.ps1` 再次输出 `VALIDATE OK`：app `65 passed / 1 ignored`、UI `26 passed`、pi-data unit `52 passed`、pi-render unit `19 passed`、live reducer `13 passed`、pi-rpc unit `12 passed` + client `15 passed`；R15 五条真实 pi 零 token T2 再次逐项通过。
- `Cargo.lock` 保持无 diff；实现提交 `534e7a3` 已推送，PR [#24](https://github.com/cking000bigdemon/GPUI-Pi/pull/24) 已创建；`ROUNDS.md` 已回填为 PR 待审。
