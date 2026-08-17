# Round 06 — 历史消息渲染（静态）

> 执行方：**Windows** · 状态：✅ 已完成

## 目标

选择一个历史会话后，在不启动 pi 会话进程的前提下后台读取共享 JSONL，并用原生 GPUI 静态展示当前会话路径上的用户、assistant、工具结果、bash、自定义消息、compaction 与 branch summary；Markdown、tree-sitter 代码高亮、diff、ANSI、图片、frontmatter 和 Mermaid 源码均可安全降级且不 panic。

## 前置

- R0–R5 已完成并合并；本轮从 `main` 的 PR #9 合并结果开始。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- R3/R5 已提供容错 JSONL 解析、24 个真实脱敏 fixture、会话列表与选择事件。
- R4 已提供正式 Dock/Root/主题/字体和 GPUI 测试链。
- UI API 以钉死 `gpui cc053a4a` 与 `gpui-component 000114aa` 的本机源码/story 为准。

## 交付物

- `crates/pi-render/src/`：静态会话 → 可渲染中间模型；Markdown/code fence、工具调用配对、unified diff、ANSI、图片元数据、frontmatter、minimap outline 与文本层快照。
- `crates/pi-render/tests/`：24 个 R3 真实脱敏 fixture 全量渲染不 panic，以及覆盖 R6 全部块类型的确定性文本快照。
- `crates/ui/src/`：原生 GPUI 消息、Markdown、工具卡片、diff、ANSI、图片与 minimap 组件；代码块使用 gpui-component tree-sitter 高亮，Mermaid 仅显示源码。
- `crates/app/src/`：历史会话后台加载状态与中心聊天面板；复用 R5 `SessionSelected`，不启动 RPC 进程。
- `Cargo.toml` / crate manifests：只接入 R6 所需且不改变上游钉版本的依赖/feature。
- 本任务卡：完成后回填实际命令、数字、限制和偏离。

## 已定语义

- **静态路径**：R6 默认渲染会话文件当前 leaf 的祖先路径；完整分支切换属于 R12。文件损坏行沿用 R3 容错 diagnostics。
- **工具配对**：assistant `toolCall` 按 id 与后续 `toolResult.toolCallId` 配对，结果不再重复作为独立消息；找不到结果时保留待完成卡片。
- **Markdown**：普通正文交给原生 `TextView`；代码 fence 在逻辑层独立分块，UI 使用 gpui-component 的 tree-sitter language features 高亮。超大块必须有上限/降级，不能冻结界面。
- **Mermaid**：按立项文档红线只以 `mermaid` 代码块源码显示，不加载 JS、不生成 SVG。
- **图片**：只接受会话中的 base64 图片元数据；无效、过大或脱敏占位数据安全显示占位信息，不 panic。真实有效图片在原生 GPUI 中解码/展示，不写临时文件。
- **Frontmatter**：消息正文开头的 YAML frontmatter 解析成原生卡片并从 Markdown 正文移除；坏 YAML 保留为普通文本。
- **ANSI**：bash 与工具文本输出识别 SGR 颜色/样式并保留纯文本；未知/截断 escape 安全降级。
- **Diff**：优先使用成功 toolResult `details.patch` / `details.diff`，解析失败时保留带行类型的原始 patch 文本。
- **Minimap**：静态对话按 user turn 建节点，assistant 的 H1–H3/首段生成可定位 outline；R6 只做选择与跳转，不做 R7 流式跟随。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 格式、clippy、单测、release 构建 | `./scripts/validate.sh` 与 `./scripts/validate.ps1` 均输出 `VALIDATE OK` |
| T1 | 纯逻辑渲染 | `cargo test -p pi-render -- --nocapture`；Markdown/code/Mermaid、tool/result、diff、ANSI、图片、frontmatter、compaction/custom/branch、未知字段均有断言 |
| T1 | 真实 fixture | 24 个 `crates/pi-data/tests/fixtures/sessions/*.jsonl` 全量加载并构建静态文档不 panic，统计实际文件/entry/block 数 |
| T1 | 文本层快照 | 确定性 snapshot 覆盖 user/assistant/thinking/tool/error/diff/bash/custom/compaction/image/frontmatter/Mermaid 与 minimap outline；换行统一 LF |
| T1 | GPUI | `cargo test -p gpui-pi chat -- --nocapture`（或等价 focused test）；loading/ready/error/empty、历史选择后台加载、Markdown、代码、工具卡、diff、ANSI、图片占位与 minimap 均可渲染，800×560 不溢出 |
| T2 | 真实只读扫描 | `PI_DATA_TEST_REAL_AGENT_DIR=<真实 agent dir> cargo test -p pi-render --test real_agent_render -- --nocapture`；扫描 ≥20 个会话、无写入、无 panic |

## 禁止

- 不实现 R7 的会话进程创建、prompt、流式 delta、abort、steer、follow-up 或滚动跟随。
- 不实现 R8 输入框、附件发送、`@文件` 或 slash 面板。
- 不实现 R10 文件浏览器/查看器；Markdown 本地链接只显示或安全打开，不在本轮新增文件 tab。
- 不实现 R11 written-files/git diff 面板；R6 仅渲染工具结果中已经存在的 patch/diff。
- 不实现 R12 分支导航、retry、compaction 操作或 HTML 导出。
- 不渲染 Mermaid 图，不引入 WebView、HTML UI、Node/npm 运行时或外部网络资源。
- 不修改 `Cargo.lock` 中已钉的 gpui/gpui-component 身份，不执行 `cargo update`，不修改 `vendor/upstream`。
- 不对真实 `~/.pi/agent` 做任何写入，不顺手修复非 R6 BACKLOG。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-06/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- `cargo test -p pi-render -- --nocapture`：exit 0；**11 个逻辑单测**、24 个真实脱敏 fixture、semantic golden 与可选真实目录测试均通过。fixture 统计为 **24 files / 1624 messages / 2668 blocks / 1903 tools / 15 images / 922 diagnostics**。922 条 diagnostics 主要来自脱敏 fixture 中统一替换后的 `toolCallId` 无法与原始 call id 配对；按设计保留为可见 orphan，没有 panic。
- `cargo test -p gpui-pi chat -- --nocapture`：exit 0；5 个 focused GPUI tests 通过，覆盖 empty、loading、ready 的 Markdown/code/Mermaid/tool/diff/ANSI/image/frontmatter/minimap selectors、error 与 stale generation。
- `cargo test -p gpui-pi -- --nocapture`：exit 0；14 个 app tests 全过，含 800×560 最小窗口布局。
- `./scripts/validate.sh`：最终 exit 0，`VALIDATE OK`；pins、fmt、workspace clippy `-D warnings`、全量 tests 与 release build 全绿。日志：`.pi/round-06-final-validate-bash.log`。
- Windows PowerShell 5.1：原 `validate.ps1` 无 BOM，直接调用会被 Windows PowerShell 按本地代码页误读中文并 parser error；未修改上轮脚本。为实际验证，生成同目录临时 UTF-8 BOM `validate` 副本，并复用已有 BOM 的 `check-pins.ps1` 临时副本，最终 validation exit 0、`VALIDATE OK`，finally 删除临时文件。日志：`.pi/round-06-final-validate-powershell.log`。
- 真实共享目录只读渲染：`PI_DATA_TEST_REAL_AGENT_DIR="$HOME/.pi/agent" cargo test -p pi-render --test real_agent_render -- --nocapture` 最终 exit 0；**scanned=176, rendered=176, blocks=15104**，测试比较全部 JSONL 的 size/modified 前后相同。日志：`.pi/round-06-post-commit-real-agent-render.log`。
- DSV4 Pro 审查发现并已修：Markdown 链接点击前使用原生 dialog 二次确认；v1 无 id 消息 fallback id 唯一；未知 ESC 后多字节 UTF-8 不再落到非法边界并补回归；普通工具与 bashExecution 的 `input_json`/output 均限长；code `TextView` id 加消息/块索引；parsed diff 显示文件路径；空白 patch/diff 不生成空块；生产加载统一走 `pi_render::render_path`；未知 JSON 的文本快照递归 canonicalize，避免 serde map feature 导致 key 顺序漂移。
- 与设计的窄化：frontmatter 仅接受文首完整 fence 与简单顶层 YAML scalar / inline tags 数组；复杂嵌套 YAML 视为坏 YAML，完整保留在 Markdown 正文。ANSI truecolor 在逻辑层完整保留 RGB，UI 为遵守“颜色全部 theme 映射”将任意 RGB 投影到 semantic theme token。图片在逻辑层限制 8 MiB base64 / 6 MiB decoded bytes并校验 MIME magic；完整像素解码仍由 GPUI image cache 异步执行，坏图显示组件 fallback，不写临时文件、不自动联网。
- 本轮仍不实现 R7 流式、R8 输入、R10 文件 tab、R11 git 面板或 R12 分支操作；Mermaid 只显示源码。
- 独立终审按约定使用 DeepSeek provider 的 `deepseek/deepseek-v4-pro`；最终结论：**终审通过，无 Blocker、High 或 Medium**。审查确认 bashExecution 静态上限与空白 patch 过滤已闭环。
- 流程偏离记录：review 修复阶段一次全量 validation 因 semantic golden 的 JSON key 顺序失败，紧接一次因 clippy `unnecessary_sort_by` 失败；按红线本应在第二次失败后写 `BLOCKED.md` 并停下，本轮未及时执行。验收标准没有放宽，随后分别用递归 canonical JSON 与 `sort_by_key` 修复，并以最终 Bash / PowerShell 全量 validation 复验全绿。后续轮次必须在第二次连续失败时立即执行阻塞流程。
