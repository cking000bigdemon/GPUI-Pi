# Round 14 — Extension UI Protocol 与资源配置

> 执行方：**Windows** · 状态：✅ 已完成（本地待提交）

## 目标

基于官方 pi 0.84.2 RPC 完整承接 Extension UI 请求，让扩展状态、widget、通知、交互式 elicitation 与编辑器控制在原生界面可用，并提供符合 v1 边界的 Skills / Plugins 配置视图；不得伪造钉死 RPC 不具备的 custom-UI 能力。

## 前置

- R0–R12 已完成并合并；R14 从 `main` 独立开发，不依赖进行中的 R13。
- 本 worktree 已独立准备 `vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/`，并通过 `scripts/check-pins.ps1`。
- R2 已定义 9 种官方 `extension_ui_request` 与 `extension_ui_response`；R7/R8 已恢复官方 extensions / skills / prompt templates 的活会话加载。
- 官方 pi 0.84.2 RPC 的 `ctx.ui.custom()` 明确直接返回 `undefined`，不会产生 custom-UI wire event；pi-web 的 headless custom-UI terminal 是其进程内 SDK 增强，不能由 GPUI-Pi 在不修改钉死内核的前提下复刻。
- UI 必须遵守 `docs/UI设计规范.md`。

## 独立性结论

- **可独立开发**：R14 所需协议、响应写回、extension 扫描与活会话事件流均在 R2/R3/R7/R8 已存在；R13 只新增分支树、compaction、retry 与 HTML 导出。
- **合并注意**：R13/R14 都可能修改 `crates/app/src/live_session.rs`、`crates/app/src/panels.rs` 与 RPC fake child，后合并者可能需要文本冲突处理；这是合并接缝，不是功能依赖。
- R14 所有活会话 UI 状态必须绑定现有 generation；会话切换、工具重启、进程退出时清空旧 dialog/status/widget，避免未来合入 R13 后旧请求污染新 session。

## 交付物

- `crates/pi-rpc/src/` 及测试：
  - 保持 9 种官方 Extension UI 请求强类型保真；提供安全、明确的 response 构造/写回路径；
  - fake child 覆盖 select / confirm / input / editor、notify、setStatus、setWidget、setTitle、set_editor_text 与 response wire；
  - 真实 pi 零 token fixture 扩展验证官方 RPC 可达能力，并明确 `custom()` 在钉死版本不可达。
- `crates/app/src/live_session.rs`、`crates/app/src/panels.rs`：
  - Extension UI 事件不再被对话 reducer 丢弃，独立进入 generation-safe pump；
  - status/widget 以 key upsert/remove，默认 widget placement 与协议一致；
  - select / confirm / input / editor 原生 dialog 顺序处理，关闭/取消必回传；notify 映射原生 notification；
  - setTitle、set_editor_text 安全投影；切换/停止/restart 清理旧 extension UI 状态；
  - widget 位于 composer 上/下方，状态栏使用 gpui-component `StatusBar`，文本做控制字符清洗和长度限制。
- `crates/pi-data/src/` 及测试：
  - 只读扫描用户/可信项目 skill 与配置 package，解析必要 frontmatter / settings 字段并保留诊断；
  - skill 的 `disable-model-invocation` 启停采用临时文件 + rename，校验允许根与文件 revision，不破坏未知 frontmatter；
  - package / plugin 只读展示来源、scope、过滤/禁用状态；不安装、不更新、不删除，不引入 npm 行为。
- `crates/app/src/resource_config.rs`（或等价模块）：
  - Skills / Plugins 原生配置 dialog，覆盖 loading / empty / error / untrusted project states；
  - Skills 支持展示与启停；Plugins 只读展示，并明确安装/更新由终端 pi 或保留的 pi-web-desktop 管理。
- `rounds/round-14/round-14.md`、`ROUNDS.md`：完成后回填本轮实测、代码审查与视觉审查结果。

## 已定语义

- **官方协议优先**：实现范围严格以 `vendor/upstream/pi-0.84.2/packages/coding-agent/src/modes/rpc/rpc-types.ts` 与 `rpc-mode.ts` 为准。
- **Dialog**：同一时刻只展示一个 extension dialog，其余 FIFO 排队；generation 变化时全部取消并清空。重复/陈旧 response 不发送。
- **Fire-and-forget**：notify / setStatus / setWidget / setTitle / set_editor_text 不回 response。
- **状态与 widget**：同 key 后到覆盖；`None` 删除；status 按 key 稳定排序；widget 未给 placement 时按官方默认 `aboveEditor`。
- **Title**：只影响当前活会话窗口标题；停止/切换时恢复 `GPUI-Pi`。
- **Editor text**：替换 composer 文本并同步当前会话草稿，不自动发送。
- **Custom UI**：记录为 `UNSUPPORTED_BY_PINNED_RPC`。不得新增私有协议、不得执行 extension component factory、不得声称实现 pi-web 进程内 headless terminal。
- **Skills**：只展示/启停，不搜索、安装、更新；符合立项文档附录 A。
- **Plugins**：只读展示配置 package/plugin；安装、更新、删除与资源过滤编辑不在本轮执行，避免引入 npm 与有损重写共享 settings。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 静态与构建 | `.\scripts\validate.ps1` 全绿（pins / fmt / clippy `-D warnings` / workspace tests / release build） |
| T2 | RPC wire | fake child 发出 9 种请求；client 解码；4 种交互请求分别收到 value / confirmed / cancelled response；fire-and-forget 不要求 response |
| T2 | 状态 reducer | status/widget upsert/remove、稳定排序、placement 默认值、控制字符清洗、generation reset、陈旧 response 拒绝均有纯逻辑测试 |
| T2 | Dialog/UI | `#[gpui::test]` 覆盖 select/confirm/input/editor 排队、取消、set_editor_text、notification、title reset、widget 上/下 placement 与状态栏 |
| T2 | 资源层 | 临时 agent/cwd fixture 覆盖 user/project/untrusted skill、frontmatter 保真启停、revision 冲突、package string/object/disabled/filter 解析；真实 `~/.pi` 仅只读扫描 |
| T2 | 真实 pi | 隔离 `PI_CODING_AGENT_DIR` 加载 extension fixture，触发官方 9 种可达请求并回传，无模型调用；另以源码/黑盒 fixture 证明 `custom()` 不发 wire event |
| T3 | 目视 | 深/浅主题检查状态栏、widget、四类 dialog、Skills/Plugins 配置视图；按项目 30 分钟截图窗口流程审查 |

## 禁止

- 不修改 `Cargo.lock`、`PINNED_PI_VERSION`、钉死 vendor 基线或上游版本；不执行 `cargo update`。
- 不依赖或 cherry-pick R13，不实现分支树、compaction、retry、HTML 导出。
- 不新增 WebView、HTML UI、xterm/浏览器终端或私有 custom-UI RPC；不修改官方 pi 二进制。
- 不在应用内执行第三方 extension component factory；不把 ANSI 控制序列直接当 GPUI 样式执行。
- 不安装/搜索/更新 skill；不安装/更新/删除 package/plugin；不嵌入或要求 Node/npm。
- 不对真实 `~/.pi` 做测试写入；共享 skill 写入仅由用户 UI 操作触发，并必须临时文件 + rename、路径校验、revision 复验。
- 不顺手修复前序轮次问题；发现后只写入 `rounds/BACKLOG.md`。
- 不创建 PR、不推送、不合并远端。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-14/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 代码审查

- 审查通道：`claude_code_review`，只读、与 writer 隔离。
- 最终 follow-up：session `87c665dd-c984-43c7-8f2e-73ae4e4fa2c2`，结论无 high / medium；确认 widget scrollbar 已采用 relative 外层 + 独立滚动 area + 同级 overlay，前轮 composer、StatusBar、Resource dialog、lifecycle reset 与 busy Skill 修复未回归。
- 接受的 low 残余风险：极端多 status 在窄窗口可能裁切；切换会话时旧 cancelled 异步写回失败可能留下误导性错误；同帧多条 Extension UI 诊断只展示最后一条。均不影响本轮协议正确性与正常验收路径，未借终审扩大产品范围。

## 视觉审查

- 视觉审查模式：`CODE_ONLY`
- 视觉审查结论：`CODE_ONLY_PASS`
- 截图验证：未提供（`SCREENSHOT_NOT_PROVIDED`）
- 兜底原因：`USER_DECLINED`
- `requested_at`：`2026-08-21T10:49:28.1429783+08:00`
- `deadline`：`2026-08-21T11:19:28.1429783+08:00`
- 审查报告：`.pi/visual-review/round-14/code-only-final-review.md`
- 补充证据 manifest：`.pi/visual-review/round-14/evidence/manifest-4ee182b95dfd4a6e.json`（9 张，均来自本次窗口）
- 说明：已完成纯代码层视觉审查，未发现 Theme token、硬编码颜色/字体、dialog footer、composer 76px / 3 行约束、widget 144px 独立滚动、StatusBar 条件渲染、Resource dialog 唯一外层滚动及可见状态测试方面的阻断项。9 张用户截图已逐张读取；最终浅色完成态可见 `Desktop review`、`runtime: Complete`，无红色错误或 timeout。但用户明确拒绝继续提供缺失的 Input dialog 与约定的 `1200x800` / `800x560` 截图，因此这些图片只作为补充证据，不能升级为 `SCREENSHOT PASS`。仅完成纯代码层视觉审查，未验证真实最小窗口或完整四类 dialog；不阻塞 PR。

## 本轮实测

- 独立性：分支 `WinClaude/round-14` 从 `main@6fee8ea` 创建，未依赖或 cherry-pick R13；worktree 独立准备 `vendor` 后，`scripts/check-pins.ps1` 全绿。
- 协议：fake child/client 覆盖官方 9 类 `extension_ui_request` 与 4 类交互 response；Select 回传原始 option，status/widget 保留 raw key identity；超限请求显式取消或拒绝，不静默截断。
- 活会话：Extension UI pump generation-safe；File tab 下仍持续投影；四类 dialog FIFO、可取消、超时、发送失败推进；人类交互请求使用 30 分钟有界 timeout，短控制请求保持 30 秒。
- 原生投影：notification、title、editor text、status 与 composer 上下 widget 均落到 GPUI/gpui-component；composer 固定 76px viewport、最多 3 行；上下 widget 各有独立 `ScrollHandle`、144px viewport 与同级 scrollbar overlay。
- 资源配置：Skills / Plugins dialog 覆盖 loading、empty、error、untrusted、trust-error、trusted、diagnostics 与长列表；Skill 启停校验 trust、允许根、canonical path、revision，并用临时文件 + rename；Plugins 保持只读。
- 真实 pi：`PI_RPC_TEST_BINARY="$PWD/vendor/pi/pi.exe" cargo test -p pi-rpc --test real_pi extension_ui_zero_token_fixture_reaches_nine_methods_and_custom_is_unreachable -- --ignored --exact --nocapture` → `1 passed`，零模型 token；同时证明钉死 RPC 中 `ctx.ui.custom()` 不产生 wire event。
- focused UI：Panels `35 passed`；ResourceConfig `6 passed`；widget 独立滚动测试在 draw 后验证 above 内容实际移动、below 不动、两个 viewport bounds 稳定。
- 全量验证：`powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1` → `VALIDATE OK`；fmt、clippy `-D warnings`、workspace tests、release build 全绿。workspace 非 ignored 测试合计 `238 passed / 0 failed`；另有需显式环境或真实凭据的 opt-in tests 保持 ignored。
- fixture：`.pi/visual-review/round-14/launch-visual-review.ps1 -ProbeOnly` 通过官方 RPC 自检；同时隔离 `PI_CODING_AGENT_DIR`、`USERPROFILE` 与 `HOME`，未读写真实 `~/.pi`。
- 视觉回归：首次截图真实暴露 30 秒交互 timeout、composer paint overflow、Resource dialog 高度塌陷；修复后补充截图确认最终完成态无 timeout/红色错误，Skills/Plugins footer 可见，深浅主题下 widget/composer/status 顺序稳定。因证据集不完整，最终严格记录为 `CODE_ONLY_PASS` 而非截图通过。
- 上游边界：官方 pi 0.84.2 独立二进制 RPC 无法实现 pi-web 的进程内 headless custom-UI terminal；本轮固定标记 `UNSUPPORTED_BY_PINNED_RPC`，未新增私有协议或伪造支持。
- 工作区：`git diff --check` 通过；staged area 为空；`Cargo.lock`、`PINNED_PI_VERSION`、`pins/`、`vendor/` 无 diff。当前未 commit、未 push、未创建 PR。
