# Round 11 — 文件浏览器、文件查看器与上传

> 执行方：**Windows** · 状态：已完成（PR #18，GitHub CI 全绿）

## 目标

在不进入 R12 git/diff 范围的前提下，为当前项目 cwd 提供原生文件树、模糊文件索引、代码/文本与图片查看器，以及安全、可恢复的文件上传；所有文件访问必须限制在项目根目录内并防止 symlink/reparse point 越界。

## 前置

- R0–R10 已完成并合并，基于 `main` 的 PR #17 合并结果。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- 新 worktree 内独立准备 `vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/`，并通过 `scripts/check-pins.ps1`。
- 功能行为以钉死的 `vendor/upstream/pi-web-0.8.9/` 为对照；UI 必须遵守 `docs/UI设计规范.md`。

## 交付物

- `crates/pi-data/src/files.rs`（或按现有分层拆分的等价文件）及测试：
  - 项目根目录约束下的目录枚举、文件读取、模糊索引；
  - 文本/图片类型识别、大小与条目数量上限；
  - symlink/reparse point 越界防护；
  - 上传预检、冲突策略（覆盖/跳过/取消）及临时文件 + rename 发布。
- `crates/ui/src/`：符合规范的文件树/文件查看器复用组件；代码与文本使用现有高亮能力，图片使用原生图片元素，不引入 WebView。
- `crates/app/src/`：
  - 当前项目的文件面板状态与异步加载；
  - 文件 tab 的打开、选择、关闭与状态隔离；
  - 原生文件选择器驱动上传，上传 busy/错误/结果反馈与重复触发防护；
  - 与 cwd/session 切换正确联动，过期异步结果不得覆盖新项目状态。
- `rounds/round-11/round-11.md`、`ROUNDS.md`：完成后回填本轮实测与本地验收状态。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 静态与构建 | `.\scripts\validate.ps1` 全绿（pins / fmt / clippy `-D warnings` / workspace tests / release build） |
| T2 | 文件访问安全 | fixture/tempdir 覆盖：根内读取成功；`..`、绝对路径注入、根外 symlink/reparse point、特殊文件与超限内容均拒绝；错误可观测且不泄漏根外内容 |
| T2 | 文件树与模糊索引 | 覆盖忽略目录、稳定排序、条目/深度/结果上限、Unicode 与 Windows 路径、查询排序；过期 generation 不覆盖新 cwd |
| T2 | 查看器 | `#[gpui::test]`/纯逻辑测试覆盖文本与代码高亮语言选择、二进制/超大文件提示、图片预览、文件 tab 打开/选择/关闭，以及缺失/变更文件的可恢复错误态 |
| T2 | 上传 | 覆盖预检、同批重名、覆盖/跳过/取消、25 MiB 单文件与 100 MiB 批次上限、原子发布、部分失败汇总；目标目录与最终路径不得越过项目根 |
| T2 | 用户路径烟测 | 在临时项目上完成“展开目录 → 打开文本/图片 → 模糊搜索 → 上传冲突处理”的自动化或可复现本地烟测，且不修改真实 `~/.pi` |
| T3 | 目视 | 本地启动应用，确认深/浅主题下文件树、tab、代码/文本、图片、busy/错误/上传反馈符合 UI 规范；本轮自动验收完成后再交用户复验 |

## 禁止

- 不修改 `Cargo.lock`、`PINNED_PI_VERSION`、钉死 vendor 基线或上游版本；不执行 `cargo update`。
- 不实现 R12 的 git status/diff、worktree 切换或 `TurnWrittenFiles`。
- 不实现 R13 的分支树、compaction、retry、HTML 导出。
- 不把文件内容自动注入对话；R8 的 `@` 补全可复用索引，但本轮不改变 prompt 语义。
- 不引入 WebView/HTML/浏览器技术栈，不执行或渲染任意主动内容（SVG/HTML 作为图片或页面执行均禁止）。
- 不跟随目录 symlink/junction/reparse point 越过项目根；不使用共享目录链接。
- 不写入真实 `~/.pi`，测试只使用临时目录。
- 不顺手修复前序轮次问题；发现后只写入 `rounds/BACKLOG.md`。
- 不创建 PR、不推送、不合并远端。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-11/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- 新 worktree 启动门禁：`fetch-pi.ps1`、`fetch-pi-source.ps1`、`fetch-pi-web.ps1` 与 `check-pins.ps1` 全绿；钉死的 pi `0.84.2`、pi-web `0.8.9`、gpui / gpui-component 身份均未漂移。
- 最终完整验收：`./scripts/validate.ps1` 于 2026-08-19 通过，退出码 `0`；实际覆盖 pins、`cargo fmt --check`、workspace clippy `-D warnings`、workspace tests、`cargo build --release`。
- GitHub：PR [#18](https://github.com/cking000bigdemon/GPUI-Pi/pull/18) 的唯一阻断 job `windows (阻断)` 于 2026-08-19 全绿（run `32206009824`，用时约 7m30s）。
- 最终测试数字：app `47 passed / 1 ignored`，UI `19 passed`，pi-data unit `38 passed`，pi-data integration `13 passed`，pi-render `30 passed`，pi-rpc `20 passed`；5 个真实 pi / 付费模型测试按显式环境门禁保持 ignored。
- 安全实测：Windows junction 逃逸稳定被拒；上传覆盖走同目录临时文件 + `MoveFileExW` 原子发布并确认无 `.tmp` 残留；预检后目标被抢占会重新拒绝；根与上传双 generation 均拒绝陈旧异步结果。
- 图片安全预算：编码体积 `10 MiB`、单边 `16384`、总解码像素 `4000 万`、GIF `200` 帧；SVG / HTML / PDF / DOCX 不执行、不渲染，明确降级为不支持预览。
- 资源与规模上限：文件树 `20,000` 条、索引记录 hard cap `50,000`、搜索索引 `5,000` 文件、深度 `8`、结果 `20`、文件 tab `16`；截断在文件面板可观测。
- 独立代码审查：使用 `claude-code-review` 多轮复审，关闭路径安全、上传竞态、tab 状态、图片解码炸弹、索引边界等阻断 finding；最终一轮仅余的两个 medium 已修复并重新完整 validation。
- 视觉证据：`.pi/visual-r11/workspace.png` 为本地 release 程序截图；自动化 UI 测试机械验证文件 Dock 默认收起、工具栏展开后具备有效宽度、文件树/搜索/上传入口及中心 tab 渲染。由于桌面环境存在其他置顶窗口，未取得可独立判定全部 R11 状态的完整截图，因此视觉门禁证据标记为 `INSUFFICIENT_EVIDENCE`，不虚报 PASS；代码层所有颜色/字体仍走 Theme token。
- 与任务卡唯一口径调整：冲突对话框禁用 Esc、右上角关闭及 backdrop 关闭，只允许显式“取消 / 跳过已有 / 覆盖普通文件”，避免状态机被隐式关闭卡死。
