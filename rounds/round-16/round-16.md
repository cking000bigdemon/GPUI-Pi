# Round 16 — 模型配置面板与登录

> 执行方：**Windows** · 状态：验收完成，待 PR / CI

## 目标

在不依赖 R13/R14/R15、不给钉死 pi 打补丁的前提下，提供原生模型配置入口：展示 provider 与认证状态，保真编辑 `models.json`，发现模型并测试连通性，安全录入/移除 API Key；OAuth/复杂登录由官方 pi 交互流程接管。

## 前置

- R0–R12 已完成并合并；R10 已提供活会话模型目录与切换通路。
- **不依赖 R13/R14/R15**：R13 只改会话树/compaction/retry/export，R14 只改扩展 UI，R15 只改项目命令环境扩展；R16 的数据边界是 `models.json` / `auth.json`、官方 pi CLI 与独立设置面板，和三轮无前置 API 或 UI 组合点。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- 本 worktree 已独立准备 `vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/`，并通过 `scripts/check-pins.ps1`；递归检查无 reparse point。
- 权威核查发现钉死 pi 0.84.2 的 `pi auth` 子命令只有 `check` / `print-api-key` / `print-bearer-token`，**没有 login 子命令**。因此登录必须 shell out 到官方 pi 的交互式 `/login <provider>`，认证完成后用 `pi auth check --provider <id> --json` 校准状态；客户端不得自行复刻 OAuth 流程。

## 交付物

- `crates/pi-data/src/model_config.rs` 及测试：
  - `models.json` 的保真模型、provider/model 增删改与校验，未知字段不丢失；
  - `auth.json` 仅解析 provider、凭据类型与掩码状态，不向 UI/日志返回密钥或 token；
  - API Key 以与上游兼容的 `{ "type": "api_key", "key": "…" }` 原子写入，移除时不误删 OAuth 凭据；
  - provider 目录与认证能力、模型发现响应解析、连通性结果模型。
- `crates/app/src/model_config.rs`（及必要的纯视图拆分）：
  - 右侧设置 sheet/原生面板入口，provider 列表、认证状态、custom provider 配置、model 列表；
  - API Key 录入、替换、移除，输入值不回显已有密钥；
  - 自定义 provider 的 base URL/API 类型、模型发现、保存与连通性测试；
  - 官方 pi 登录按钮：在新终端中仅启动钉死的官方 `pi.exe` 交互 TUI，面板明确提示用户手动输入 `/login <provider>`，终端返回后刷新认证状态；
  - busy/generation 防陈旧覆盖、成功/错误通知、深浅主题 token 与可访问 tooltip。
- `crates/app/src/model_service.rs`（或等价非 UI 模块）及测试：
  - 运行 `pi --list-models` / `pi auth check` 的零 token探测；
  - 对 OpenAI/Anthropic/Google 兼容 `/models` 端点进行有超时、有限响应体、密钥不进 URL/错误文本的模型发现；
  - 对选中模型执行最小、可取消、明确提示可能产生请求的连通性测试；不把测试失败写进配置。
- `rounds/round-16/round-16.md`、`ROUNDS.md`：完成后回填实测、代码审查与视觉审查结果。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 静态与构建 | `.\scripts\validate.ps1` 全绿（pins / fmt / clippy `-D warnings` / workspace tests / release build） |
| T2 | 配置往返 | 临时 agent dir 覆盖空文件、未知字段、自定义 provider/model、partial cost、非法结构、并发替换前校验；保存后读回语义一致且无真实 `~/.pi` 写入 |
| T2 | 凭据安全 | API Key 原子写入/替换/删除；OAuth 类型不被 API Key 删除路径误删；错误、Debug 与 UI 状态不包含 secret；文件写权限使用项目现有原子写语义 |
| T2 | 发现与测试 | 本地 mock HTTP 覆盖 OpenAI/Anthropic/Google 模型列表、401/403/429/5xx、超时、超大响应、畸形 JSON、重复模型；Authorization 只走 header |
| T2 | 官方 pi CLI | 临时 `PI_CODING_AGENT_DIR` 下运行 `pi auth check --json` 与 `pi --list-models`，零 token、不触碰真实凭据；登录命令构造测试证明 provider 不经 shell 拼接 |
| T2 | UI/状态 | `#[gpui::test]` 或纯逻辑测试覆盖 sheet 开关、provider 选择、busy 禁重入、stale generation、secret 清空、保存/发现/测试/登录错误态与最小窗口布局 |
| T3 | 用户路径 | 至少两家 provider 完成真实 API Key 或 OAuth 登录；刷新目录后能选择模型并进行一次明确授权的连通性测试 |
| T3 | 目视 | 深/浅主题下检查设置入口、provider 列表、表单、错误/空态、窄窗口与 secret 输入，按 30 分钟截图流程完成视觉审查 |

## 禁止

- 不修改 `Cargo.lock`、`PINNED_PI_VERSION`、钉死 vendor 基线或上游版本；不执行 `cargo update`。
- 不依赖或合并 R13/R14/R15 的未完成分支，不实现它们的会话树、Extension UI Protocol 或项目 bash 扩展。
- 不给 pi 打补丁，不在客户端实现 OAuth/PKCE/device-code 回调服务；复杂登录只交给官方 pi 交互式 `/login`。
- 不把 API Key/token 放进命令行参数、URL、日志、通知、Debug 输出或持久化 UI state；不读取/展示真实凭据正文。
- 不在测试中修改真实 `~/.pi`；所有配置、认证与 CLI 测试使用临时 `PI_CODING_AGENT_DIR`。
- 不引入 WebView/HTML/Node/Python；不绕过 `docs/UI设计规范.md` 硬编码颜色、字体或浮层坐标。
- 不顺手修复前序轮次问题；发现后只写入 `rounds/BACKLOG.md`。
- 不创建 PR、不推送、不合并远端。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-16/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 视觉审查

- 代码审查结论：APPROVE（最终独立 Claude Code review，findings=0；session id `d7a84502-a73c-4c12-8afb-80bb58c92e0c`）
- 视觉审查模式：SCREENSHOT
- 视觉审查结论：PASS
- 截图验证：已提供 5 张（SCREENSHOT_PROVIDED）；用户明确“就这5张，够了”
- 兜底原因：N/A
- `requested_at`：`2026-08-20T14:08:13.1068366+08:00`
- `deadline`：`2026-08-20T14:38:13.1068366+08:00`
- 证据消息：entryId `1c5a7105`，messageTimestamp `2026-08-20T14:17:28.505+08:00`，截止前回传
- 证据 manifest：`D:/variFlight_work/GPUI-Pi-round-16/.pi/visual-review/round-16/evidence/manifest-64c6924f394d28e7.json`；`actualImageCount=expectedImageCount=5`
- 审查报告：`D:/variFlight_work/GPUI-Pi-round-16/.pi/visual-review/round-16/visual-review-final.md`
- 已验证：宽屏浅色/深色主题、认证/未认证、API 类型 warning、Provider ID mismatch 禁用、登录指引与 success notification；无阻断视觉 findings。
- 非阻断残余：未实拍 800×560 最小窗口和 JSONC 重写 warning 像素态；代码已有 800×560 GPUI 测试，不宣称这两项已完成截图验证。

## 本轮实测

- 数据层：`model_config` focused tests 18 项通过。`write_api_key` 改为字段级更新，保留既有 `env` 与未知字段，拒绝覆盖 OAuth/未知非 api_key 类型；`remove_api_key` 只移除 `key`，env/profile/ADC/AWS/未知字段仍保留，仅纯 `{type,key}` 才删除整条。auth summary 区分 env-only 配置与真正存在的可移除 key。其余 revision、未知 API、JSONC trivia、literal secret 与外部引用测试继续通过。
- CLI / 网络：`model_service` focused tests 16 项通过。新增 `InvalidProviderId` / `InvalidArgument`，provider 白名单错误明确列出允许字符，内部 curl/header 参数校验不再误报服务响应格式；其余官方 literal secret、loopback HTTP、stdout/curl/chunked 测试继续通过。
- 登录：系统绝对路径 `cmd.exe` + `start /wait` 只启动官方 pi 交互 TUI，pi argv 不再包含 `/login` 或 provider prompt，避免 initial prompt 产生计费请求。面板常驻文案、启动前通知和返回通知均要求用户在终端内手动输入精确 `/login <provider>`；等待仍可取消且最长 15 分钟，退出后无论退出码均执行 `auth check --no-refresh` 校准。
- UI / 状态：`model_config` focused tests 9 项通过。Discover/Test/Login/API Key 四类 provider-bound 操作统一复用 selected provider 与 Provider ID 输入一致性守卫；不一致时按钮禁用且直接调用在读取密钥、构造请求前失败，统一提示先保存。连通性只有 Reachable 使用 success，AuthenticationRequired / RateLimited / ServerError 使用 warning。env-only auth 不显示为可移除 Key，`remove_api_key Ok(false)` 使用中性提示并 refresh；unsupported API 文案继续区分未知原值与缺失值。
- focused validation：`cargo test -p pi-data model_config -- --nocapture`（18 passed，exit 0）；`cargo test -p gpui-pi model_service -- --nocapture`（16 passed，exit 0）；`cargo test -p gpui-pi model_config -- --nocapture`（9 passed，exit 0）；`cargo clippy -p gpui-pi -p pi-data --all-targets -- -D warnings`（exit 0）；`cargo fmt --all -- --check` 与 `git diff --check`（exit 0）。
- 全量 validation：`powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1` exit 0；pins、fmt、clippy、workspace tests（app 82 passed / 1 ignored；ui 24；pi-data 65 + integration；pi-render 19 + integration；pi-rpc 8 + client 12）与 release build 全绿，末行 `VALIDATE OK`。
- 视觉 SCREENSHOT review：已完成并 PASS，5 张截图证据与审查报告见上方记录。
- T3 用户路径：用户于 `2026-08-20` 确认 `cliproxy-dmit` 与 `deepseek` 两家真实 provider 的 API Key/OAuth 认证状态均正常；刷新/发现后可看到并选择真实模型，且已完成至少一次明确授权的真实连通性测试，结果 OK。未记录、展示或提交任何密钥/token 正文。
- 收口状态：T1–T3、独立代码审查与 SCREENSHOT 视觉审查均通过；待创建 PR 并提交 GitHub CI。
