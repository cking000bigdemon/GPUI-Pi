# Round 04 — 主界面框架

> 执行方：**Windows** · 状态：✅ 已完成

## 目标

让正式 `gpui-pi` 二进制打开一个可继续演进的原生主窗口：具备 DockArea 左侧栏与中心工作区、标题栏下方的应用 TabBar/工具栏、原生窗口控制标题栏、系统深浅主题实时跟随、内嵌图标与开源 CJK 字体，以及只返回内存路径的系统目录选择器。

## 前置

- R0–R3 已完成并合并；`main` 位于 PR #6 合并结果。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- UI API 以钉死 `gpui cc053a4a` 与 `gpui-component 000114aa` 的本机源码/story 为准。
- R1 四项风险门禁已经人工通过；本轮将移除 spike 可执行物，但保留 `rounds/round-01/round-01.md` 的历史证据。

## 交付物

- `crates/app/src/main.rs`：正式 GPUI 应用入口、窗口生命周期、资源与主题初始化。
- `crates/app/src/workspace.rs`：标题栏、DockArea、目录选择状态和运行时主题跟随编排。
- `crates/app/src/panels.rs`：R4 侧栏/中心占位 Panel；不加载真实会话。
- `crates/ui/src/theme.rs`：内嵌 Noto Sans SC 字体加载、平台字体策略和系统主题同步。
- `crates/ui/src/tab_bar.rs`：可复用的应用级单 Tab 容器。
- `crates/ui/src/shell.rs`：使用 Theme token 的主窗口外壳。
- `crates/ui/assets/fonts/`：OFL 许可的 Noto Sans SC 字体与许可证。
- `crates/app/Cargo.toml`：移除 R1 spike bin 与 spike-only 依赖。
- 删除 `crates/app/src/bin/spike.rs`、`crates/app/src/spike_data.rs`、`scripts/measure-spike-cold-start.ps1`。
- `#[gpui::test]`：真实渲染后的标题栏、侧栏、中心区 bounds 与窄窗口断言。
- 本任务卡：完成后回填实际命令、数字、目视结果、踩坑与偏离。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 格式、clippy、单测、release 构建 | `./scripts/validate.sh` 与 `./scripts/validate.ps1` 均输出 `VALIDATE OK` |
| T1 | 布局测试 | `cargo test -p gpui-pi workspace::tests -- --nocapture`；固定窗口与最小窗口均验证标题栏/交互工具栏在顶、左侧栏非零且位于中心区左侧、中心区保留可用宽度 |
| T1 | 资源与主题 | 内嵌字体可由 GPUI text system 注册；主题初始化读取窗口 appearance，并由 `observe_window_appearance` 在运行中同步 |
| T3 | Windows 目视 | 深/浅系统主题切换实时同步；标题栏拖动/双击最大化/最小化/最大化/关闭正常；100%/125%/150% DPI 下中文、图标、Tab 与 Dock 无截断/溢出；目录选择取消不 panic，Unicode 路径正确显示 |

## 禁止

- 不实现 R5 会话列表、运行状态、trust UI、重命名/删除/导出；侧栏只有容器和占位内容。
- 不实现 R6+ 的消息渲染、输入框或 RPC 会话接线。
- 不保存 Dock 布局，不写真实 `~/.pi/agent`，目录选择结果只存内存。
- 不引入 WebView、HTML、Web 图标或字体 CDN。
- 不嵌入微软雅黑/Segoe UI 等专有字体文件；只使用随仓库附带许可证的 OFL 字体。
- 不修改 `Cargo.lock` 上游钉版本，不运行 `cargo update`。
- 不顺手修复不属于 R4 的 BACKLOG 项。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-04/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- 正式 `gpui-pi` 入口已从 R0 CLI 自检替换为 GPUI 原生窗口；默认窗口 1280×820、最小 800×560，`TitleBar::window_options()` 提供原生拖动、双击与窗口控制区域。
- 主布局使用真实 `DockArea`：左 Dock 初始 **280px**、可折叠/拖动；中心区占余量。R4 只放占位 Panel，没有读取会话、连接 RPC 或写入共享 `~/.pi/agent`。
- 应用级 `TabBar` 与“打开目录”按钮位于标题栏下方的 **38px 交互工具栏**。独立审查发现 Windows 上 `TitleBar` 内容区整体属于原生 Drag hitbox，已据此把交互控件移出标题栏，避免点击被解释成拖窗。
- 原生目录选择器使用 `PathPromptOptions { files: false, directories: true, multiple: false }`；取消安全返回，路径只保存在内存。自动化点击测试验证按钮能发起 native path prompt，并覆盖取消、盘符根目录与中文目录标签。
- 主题在窗口创建时读取 `window.appearance()`，并持有 `observe_window_appearance` subscription 做运行中同步；每次切换后重放字体策略，同时更新 `gpui-component::Theme` 与 `gpui-base::Theme` 的 typography 投影。
- 图标使用 `gpui-component-assets` 内嵌 SVG；字体内嵌 OFL 许可的 **Noto Sans SC Regular 1,213,236 bytes**，Windows 正文仍按约定优先 `Microsoft YaHei UI`、等宽 `Consolas`，未提交专有字体文件。
- app 最终 **5 个测试用例**：其中 **4 个 `#[gpui::test]`** 覆盖 1200×800 默认布局、800×560 最小布局、侧栏连续收起/展开、目录按钮 native prompt，并通过正式 `Root` 和字体初始化链，多轮 draw 后读取 `debug_bounds` 与 `on_prepaint` 实际 bounds；另有 **1 个纯函数单测**覆盖根目录/中文目录标签。`gpui-pi-ui` 另有 **3 个**字体/主题单测。
- R1 spike 已完成清理：删除 `crates/app/src/bin/spike.rs`、`crates/app/src/spike_data.rs`、`scripts/measure-spike-cold-start.ps1`，移除 `spike` bin、`gpui-fps`、`instant` 和 app 对 `pi-data` / `pi-rpc` 的未使用直依赖；历史门禁证据保留在 R1 任务卡。
- 最终 `target/release/gpui-pi.exe` 约 **19MB**；内嵌字体约 **1.2MB**。尚未包含 R16 的 `vendor/pi` 与安装包。
- 最终 validation：`./scripts/validate.sh --logic` 与 `./scripts/validate.sh` 均输出 `VALIDATE OK`。Windows PowerShell 5.1 通过临时 UTF-8 BOM 副本实跑 `scripts/validate.ps1`，同样输出 `VALIDATE OK`；原脚本哈希与 `HEAD` 一致，没有借验收改脚本。
- release smoke：在 Windows 11 / AMD Radeon 780M / Direct3D 11.1 上启动并保持运行，日志无 panic/error，DirectWrite 明确使用 `Microsoft YaHei UI`；`PrintWindow` 截图可见正式浅色界面、标题栏、Tab、侧栏、中心区与图标。UI Automation 枚举到 Tab、TabItem 与“打开目录” Button，并成功调用 Button 的 Invoke pattern；自动化与目视附件位于 gitignored `.pi/r4-window-print.png`、`.pi/r4-uia.txt`。
- T3 Windows 人工复核已完成：系统深浅主题实时同步、原生标题栏拖动/双击及最小化/最大化/关闭、原生目录选择和取消均通过；150% DPI 下中文、Tab、Dock 与图标无截断/溢出。首轮复核发现工具栏与侧栏标题图标过于贴左、组件库内置 Dock 收展按钮点击无响应；已把统一的应用级侧栏按钮移到工具栏，使用 12 logical px 左右 padding，移除重复装饰 icon，并由用户复核确认 150% DPI 间距及侧栏拖动/收起/展开全部通过。
- 独立审查：按约定先核对 reviewer runtime，并使用 **DeepSeek provider 的 `deepseek/deepseek-v4-pro`** 完成多轮只读审查。主审与 T3 修复后终审均明确 **无 Blocker、无 High**；标题栏 Drag hitbox、最小窗口断言、正式 Root/字体测试链、base typography、根目录标签、工具栏重绘、重复 Dock 按钮与收展测试等审查意见均已修正。
