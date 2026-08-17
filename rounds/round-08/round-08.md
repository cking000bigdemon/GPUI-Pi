# Round 08 — 输入框 / 附件 / slash 面板

> 执行方：**Windows** · 状态：✅ 已完成（T1/T2；T3 待人工复测）

## 目标

把 R7 的基础 Textarea 产品化为真实可日用 composer：多行/IME 安全输入，当前会话 cwd 内的 `@文件` 补全，来自官方 pi `get_commands` 的 slash 面板，图片选择/粘贴/拖拽附件，以及按会话隔离的草稿恢复；文本或图片均可通过既有 `prompt + streamingBehavior` 发送且明确拒绝时不丢稿。

## 前置

- R0–R7 已完成并合并；本轮从 `main` 的 PR #13 合并结果开始。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- `vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/` 已在本 worktree 独立准备并通过 `scripts/check-pins.ps1`。
- R7 已提供真实 Textarea、`Prompt`/abort/steer/follow-up 和活会话 reducer；R2 wire type 已支持 `ImageContent` 与 `GetCommands`。
- 项目资源继续遵守 R5 的共享 `trust.json` 门禁；未信任资源由官方 pi 忽略，不由客户端绕过。

## 交付物

- `crates/pi-data/src/composer.rs`：不依赖 GPUI 的 `@` token/插入/fuzzy 排序、bounded 文件索引、图片格式/大小校验和进程内按 session key 草稿 store。
- `crates/pi-data/src/lib.rs` / `Cargo.toml`：导出 R8 composer 纯逻辑 API；不改共享 pi 配置 schema，不写 session JSONL。
- `crates/app/src/live_session.rs`：提交结构携带图片；slash commands 在后台通过 `get_commands` 加载；活会话启用官方 extensions/skills/prompt templates，使面板返回项真实可执行。
- `crates/app/src/panels.rs`：输入菜单、键盘选择、附件缩略图/删除、文件选择、剪贴板图片、Explorer 拖拽、会话草稿切换与失败恢复。
- `crates/pi-rpc/tests/`：fake child 覆盖 `get_commands` 强类型解码及带图片 prompt wire 保真。
- `crates/pi-data` / `crates/app` focused tests：覆盖纯逻辑、异步 stale generation、图片-only、失败恢复、popup/最小布局与 IME 不误发的可自动化部分。
- 本任务卡：完成后回填命令、测试数字、实际限制与 T3 结果或待人工项。

## 已定语义

- **多行与 IME**：继续使用经 R1 验证的 gpui-component `TextareaState`；桌面 Enter 发送、Shift+Enter 换行。组合态由原生 input engine 吞掉，composer popup 不在 marked text 期间抢占 Enter/Tab/方向键。
- **发送与恢复**：文本会 trim 后发送；纯图片允许发送。点击发送后 UI 清空并继续接收后续输入；只有 RPC 明确 `success:false` 时，把失败提交置于当前新草稿之前恢复，图片同样恢复且总数不超过 10。超时/进程退出属于“是否已接受不明确”，不自动恢复，避免重复 turn，并显示错误。
- **图片**：最多 10 张，每张原始 bytes ≤ 10 MiB；仅接受跨 provider 稳定交集 PNG/JPEG/GIF/WebP，MIME 由实际字节/GPUI image format决定，不信任扩展名。RPC 使用无 data URL 前缀的 base64。入口为原生多选文件、剪贴板图片和 Explorer 文件拖拽；非图片/超限文件显示明确提示。
- **`@文件`**：仅当前 session cwd；`@` 必须在文本开头或前一字符为空白，支持 `@path` 与 `@"path with spaces"`。候选最多 20，目录可继续 drill-down，文件完成后插入空格。补全只插入 cwd 相对路径文本，不读取/上传文件内容，也不复刻 CLI 启动参数的 `@file` 处理。
- **文件索引**：后台构建；优先 `git -C <cwd> ls-files --cached --others --exclude-standard -z`，失败后 bounded BFS；客户端索引最多 5000 文件，walk 深度/数量有硬上限，不跟随 symlink/reparse point，不在输入回调或 render 内同步递归扫描。
- **Slash**：只展示官方 RPC `get_commands` 返回的 extension / prompt / skill，按 source + 名称稳定排序；输入首 token `/query` 且命令名内无空白时打开，Tab/Enter仅补成 `/<name> `，不立即执行。R9/R12 才实现的模型、compact/reload 等客户端 built-in 不在本轮冒充 RPC command。
- **资源加载**：移除 R7 临时的 `--no-extensions --no-skills --no-prompt-templates`；保留 `--no-context-files`，上下文文件行为不属于 R8。项目资源是否可加载仍由官方 trust 解析决定。
- **草稿**：与 pi-web 0.8.9 对齐为**进程内**、按 session id/path 隔离的 store；切会话保存旧稿并恢复新稿，成功发送清除该 key。应用重启恢复不是本轮承诺；不写 `~/.pi`、`settings.json` 或 session JSONL。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 格式、clippy、单测、release 构建 | `./scripts/validate.sh` 与 `./scripts/validate.ps1` 均输出 `VALIDATE OK` |
| T1 | composer 纯逻辑 | `cargo test -p pi-data composer -- --nocapture`；覆盖 `@` 触发/空格 quoting/目录 drill-down/fuzzy、git/walk 索引上限、图片 magic/10MiB/10 张、草稿切换/恢复合并 |
| T1 | RPC / app focused | `cargo test -p pi-rpc -p gpui-pi composer -- --nocapture`（或等价 focused tests）；fake child `get_commands` 解码、图片 prompt、slash/@ popup、图片-only、明确拒绝恢复、stale async result、800×560 composer 不溢出 |
| T2 | 官方 pi 零 token | `PI_RPC_TEST_BINARY="$PWD/vendor/pi/pi.exe" cargo test -p pi-rpc --test real_pi -- --ignored --nocapture`；既有命令矩阵/restart/resume 继续全绿，并确认 `get_commands` response 可解码（允许当前隔离目录返回空列表） |
| T2 | 真实资源/图片 wire | 在隔离 `PI_CODING_AGENT_DIR` 放置临时 prompt/skill fixture，启动官方 pi 后 `get_commands` 返回且 `/fixture` 可被 prompt 接受；图片发送测试默认只到 fake child，不烧 token，真实模型图片仅显式 opt-in |
| T3 | Windows 真机输入 | 微软拼音/搜狗在真实 composer 中候选窗正确、组合期不丢字，Enter 只提交候选不误发；Shift+Enter 换行，菜单打开时方向键/Tab/Enter 可用且组合态不抢键 |
| T3 | Windows 真机附件 | 文件选择、截图粘贴、Explorer 拖入 PNG/JPEG 均显示缩略图、可删除、可发送；超限/不支持格式提示清楚；切换会话草稿与附件不串稿 |

## 禁止

- 不实现 R9 模型/思考级别/工具预设，或 R12 的 compact/reload/session/copy 等客户端 built-in slash 行为。
- 不实现 R10 文件浏览器/查看器/上传；`@` 仅是当前 cwd 的文字补全，不读取文件内容、不全盘搜索。
- 不实现图片编辑、OCR、压缩/转码工作流或远程 URL 附件。
- 不新增跨应用重启的磁盘草稿，不写共享 `settings.json` schema、不 append/重写 session JSONL。
- 不引入 WebView、HTML UI、Node/npm 运行时或外部网络资源。
- 不执行 `cargo update`，不修改 `Cargo.lock` 中上游钉身份，不修改 `vendor/upstream`。
- 不破坏性写真实 `~/.pi/agent`；T2 fixture 一律使用隔离临时 `PI_CODING_AGENT_DIR`。
- 不顺手修复非 R8 BACKLOG；发现跨轮次问题只登记。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-08/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- `./scripts/check-pins.sh`：exit 0；pi 0.84.2 / pi-web 0.8.9 / GPUI 锁定身份与 manifest 全绿，未修改 `vendor/upstream`、未执行 `cargo update`。
- `cargo test -p pi-data composer -- --nocapture`：exit 0，8 passed；覆盖 `@` 触发/quoted path/目录 drill-down/fuzzy、UTF-8 中间光标插入且保留后续文本、git bounded 收集/truncated、bounded walk、图片 magic/10 MiB/10 张、按 session key 草稿与明确拒绝 prepend 恢复。
- `cargo test -p pi-rpc --test client -- --nocapture`：exit 0，10 passed；fake child 强类型解码 extension/prompt/skill，并逐字段校验无 data URL 前缀的图片 prompt 与 `streamingBehavior`，既有并发/restart/burst/queue 回归全绿。
- `cargo test -p gpui-pi -- --nocapture`：exit 0，22 passed；新增 app-layer UTF-8 中间光标 `@` 插入、目录接受后立即 drill-down popup、会话草稿切换隔离，以及失败 response 仅在尚未观测 `AgentStart` 时回退 Idle；slash query、图片-only submission、明确拒绝/歧义失败、popup/附件/原生文件选择和既有聊天布局/最小 composer/stale generation 回归全绿。
- `cargo fmt --all -- --check`：exit 0。
- `cargo clippy -p pi-data -p pi-rpc -p gpui-pi --all-targets -- -D warnings`：exit 0。
- 活会话仅保留 `--no-context-files`，后台加载 `get_commands`；extension/skill/prompt template 恢复官方默认加载，是否加载项目资源仍由官方 trust 门禁决定。
- 图片入口实现原生多选文件、GPUI clipboard image 与 `ExternalPaths` drop；文件读取/索引/RPC 均在后台。MIME 以实际 magic bytes 校验，仅 PNG/JPEG/GIF/WebP，最多 10 张、每张 10 MiB。
- 失败语义：仅收到 `success:false` 的明确拒绝才把文本/图片恢复到当前新草稿前；timeout/进程退出等歧义失败明确提示且不恢复。
- 官方 pi 零 token：`PI_RPC_TEST_BINARY="$PWD/vendor/pi/pi.exe" cargo test -p pi-rpc --test real_pi -- --ignored --nocapture` exit 0，5 passed；`get_commands` 已改为 `CommandsData` 强类型解码并允许隔离目录按官方加载规则返回非空资源。
- `./scripts/validate.sh`：exit 0，`VALIDATE OK`；pins、fmt、workspace clippy `-D warnings`、全量 tests 与 release build 全绿。关键计数：`gpui-pi` 22 passed、`gpui-pi-ui` 4 passed、`pi-data` 27 unit + 13 integration passed、`pi-render` 14 unit + 4 integration passed、`pi-rpc` 8 unit + 10 client passed。日志：`.pi/round-08-validate-bash.log`。
- Windows PowerShell 5.1：仓库既有 `validate.ps1` 无 UTF-8 BOM，直接执行仍会按本地代码页误读中文并 parser error；这是 R6/R7 已记录的前轮脚本问题，本轮未跨轮修改。生成同目录带 BOM 的 gitignored 临时副本后，全量 validation exit 0、`VALIDATE OK`，finally 已删除临时文件。日志：`.pi/round-08-validate-powershell.log`。
- 官方 pi 隔离资源 T2：在临时 `PI_CODING_AGENT_DIR` 放置 extension、prompt template、skill fixture，运行钉死 `vendor/pi/pi.exe --mode rpc --no-session --no-context-files --offline`；`get_commands` 返回 `fixture-extension`、`fixture-prompt`、`skill:fixture-skill`，随后 `/fixture-extension` 通过 `prompt` 返回 `success:true`。原始 JSONL：`.pi/round-08-resource-fixture.jsonl`。
- 独立审查按约定显式使用 DeepSeek provider 的 `deepseek/deepseek-v4-pro`。首轮发现 `@` 补全忽略真实光标位置；已修为使用 `TextareaState::cursor()`，同时补目录即时 drill-down、git 索引硬上限及 prompt response/AgentStart phase 竞态回归。最终复审结论：**终审通过，无 Blocker、High 或 Medium**。
- 未做 T3 真机人工验收；未声称 T3 通过。特别保留验证点：微软拼音/搜狗真实组合输入，以及 popup 打开时 Up/Down 是否会同时移动 Textarea 光标与候选。钉死 API 虽有 capture phase，但没有公开的 IME marked/composing 查询，改为 capture interceptor 可能抢占输入法候选键，因此本轮未做危险拦截。
- 范围保持：草稿仅进程内，不写共享 `~/.pi`；`@` 只插入 cwd 相对路径文字；slash 仅展示官方 `get_commands` 的 extension/prompt/skill；未实现 R9/R10/R12 功能，未修改 `vendor/upstream`，未执行 `cargo update`。
