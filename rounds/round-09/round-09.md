# Round 09 — 前端视觉打磨（依据《UI 设计规范》）

<!-- 保存为 rounds/round-09/round-09.md；该轮其他管理产出也放在同一目录。 -->

> 执行方：**Windows** · 状态：🟡 实现完成，T1/T2 已过，待 T3 人工目视验收

## 目标

依据 [`docs/UI设计规范.md`](../../docs/UI设计规范.md)（由 Zed 设计语言调研映射 gpui-component 形成，R9 前置任务产出），对 GPUI-Pi 现有界面逐项做视觉打磨，使消息区、侧栏、composer、目录的观感对齐规范。**只允许动 UI 代码，不允许动业务代码**（边界见「禁止」）。

## 前置

- R0–R8 已完成并合并；本轮从 `main` 的 PR #15 合并结果开始。
- **`docs/UI设计规范.md` 已存在且通过评审**（若缺失或与 gpui-component 实际 token 不符，先补齐再开工）。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`；`vendor/` 三件套已独立准备并通过 `scripts/check-pins.ps1`。
- 现有 `#[gpui::test]` 布局断言基建（`debug_selector` / `debug_bounds` / `LayoutProbe`）可用，本轮在其上新增视觉结构断言。

## 交付物

按规范文档的章节逐项落地，覆盖但不限于：

- `crates/ui/src/chat.rs`：
  - `MessageView`：去掉整条消息的外层大边框卡片，改为规范规定的消息条目节奏（间距/圆角/角色标识/用户-助手区分方式）。
  - `render_thinking` / `render_tool` / `render_diff` / `render_code` / `render_ansi` / `render_image` / `render_frontmatter` / `Block::Notice`：统一弱边框、header 状态点、左竖线从属表达、背景档位。
  - `ChatMinimap`：选中项样式（竖条/弱底）按规范。
- `crates/app/src/session_sidebar.rs`：会话行操作按钮 hover 显隐（或收进 Popover 菜单）、元信息拆两行、行选中/悬停态按规范。
- `crates/app/src/panels.rs`（仅 composer 视觉区）：按钮主次分层（发送 primary 最右、Steer/Follow-up 合并 ToggleGroup、停止按运行态出现）、composer 容器背景层级。
- `crates/ui/src/theme.rs`：若规范要求且 gpui-component 默认 token 不足，只在本文件集中定义 token 投影；禁止组件内硬编码颜色。
- 测试：为每项改造补 `#[gpui::test]` 视觉结构断言（见验收 T2）。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 静态 | `.\scripts\validate.ps1` 全绿：fmt / `clippy -D warnings` / 全部单测 / `cargo build --release` |
| T2 | 视觉结构断言 | 新增 `#[gpui::test]` 覆盖：① 消息根节点无外层边框（结构性或快照断言）；② 工具卡片 header 状态点存在（`debug_selector`）且整卡边框为 border 系；③ 侧栏 hover 前操作按钮不可见、hover 后可见；④ composer ToggleGroup 选中态切换；⑤ minimap 选中项竖条存在。既有全部测试不回归 |
| T3 | 目视 | 人工对照《UI 设计规范》逐项目视：消息区无「框套框」、工具/思考卡片弱边框 + header 状态点、侧栏操作 hover 显隐、composer 主次分明；深/浅两种模式各看一遍，结论回填本轮实测 |

## 禁止（UI/业务边界）

- **不改** `pi-rpc` / `pi-data` / `pi-render` 任何代码与模型结构（`Block` / `ToolStatus` / `DiffLineKind` / 渲染中间模型等保持原样）。
- **不改功能行为**：点击、折叠、发送、滚动跟随、会话切换、草稿恢复、附件等交互逻辑只允许调整**视觉呈现与布局位置**，不允许改变语义与触发条件。
- **不引入新依赖 crate**；不引入 web 技术栈。
- **不在组件内硬编码颜色/字体**——全部走 `cx.theme()` token（规范文档映射表）。
- 不顺手改其他轮次问题（记 `rounds/BACKLOG.md`）。
- 不复制 Zed 代码（其组件体系不同，只能遵循规范文档的映射结论）。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-09/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

> 执行环境：Windows 11 Pro 26200 · Windows PowerShell 5.1 · rustc 1.97.1-x86_64-pc-windows-msvc
> 改动范围：`crates/ui/src/chat.rs`、`crates/app/src/session_sidebar.rs`、`crates/app/src/panels.rs`（三个纯逻辑 crate 与 `crates/ui/src/theme.rs` 均未改动）

### 门禁

本 worktree 内独立准备 `vendor` 三件套并全绿：`vendor\pi\pi.exe` (0.84.2)、`vendor\upstream\pi-0.84.2\`（1373 文件对基线全量一致）、`vendor\upstream\pi-web-0.8.9\`（380 文件一致）；`check-pins.ps1` 十项全 OK；worktree 内递归扫描无任何 reparse point（红线 6）。

### T1 静态

**`validate.ps1` 本机无法执行**——脚本缺 UTF-8 BOM，Windows PowerShell 5.1 按 GBK 解码其中文注释后整个文件解析失败（`TerminatorExpectedAtEndOfString`），一步都进不去；本机没有 pwsh 7（BACKLOG #1 提到的那台已随 Windows solo 退役）。属 R0 脚本问题，按红线 3 记入 `rounds/BACKLOG.md` #6，本轮不改，改为**按脚本定义逐条执行同样的五步**，验收内容未打折：

| 步 | 命令 | 结果 |
|---|---|---|
| 1 | `.\scripts\check-pins.ps1` | OK（10 项） |
| 2 | `cargo fmt --all -- --check` | OK |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | OK，无 `#[allow]` |
| 4 | `cargo test --workspace` | 全绿；新增断言后 `gpui-pi` 31 passed、`gpui-pi-ui` 10 passed，其余 crate 未受影响 |
| 5 | `cargo build --release --workspace` | OK |

`pi-rpc/tests/real_pi.rs` 的 5 个用例仍是 opt-in ignored（需 `PI_RPC_TEST_BINARY` / `PI_RPC_R7_*`），与本轮无关。

### T2 视觉结构断言

| 项 | 用例 | 位置 |
|---|---|---|
| ① 消息根节点无外层边框 | `only_user_messages_are_carded` + `chat_renders_message_flow_and_tool_card` | `crates/ui/src/chat.rs` |
| ② 工具卡状态点 + 中性边框 | `tool_card_header_has_status_dot`、`tool_card_border_is_neutral_for_every_status` | 同上 |
| ③ 侧栏操作 hover 显隐 | `session_row_actions_are_hover_only`（hover 前三个按钮 `debug_bounds` 均为 `None` → hover 后均为 `Some` → 移出再次 `None`） | `crates/app/src/session_sidebar.rs` |
| ④ composer ToggleGroup 选中态切换 | `composer_mode_toggle_group_switches_selection`（真实点击两段来回切换）+ `toggle_group_checks_are_reduced_to_a_single_mode` | `crates/app/src/panels.rs` |
| ⑤ minimap 选中项竖条 | `minimap_selected_node_has_accent_bar`（未选中 `None`、选中 2px 竖条） | `crates/ui/src/chat.rs` |

附带新增：`metrics_split_into_two_short_lines`（元信息每行 ≤3 片段）、`abort_button_is_absent_until_running`。

既有用例无回归；只有一处按新规范**改写**而非放宽：原 `ready_sidebar_renders_project_row_metrics_and_actions` 断言「行操作按钮常驻可见」，与 T2 ③ 直接冲突，已拆成 `ready_sidebar_renders_project_row_and_diagnostics`（行/诊断仍断言）+ `session_row_actions_are_hover_only`（新行为）。

### 踩的坑

1. **`ToggleGroup` 不是单选控件**。它把被点那一段取反后回传**整个**勾选向量，所以 Steer 选中时点 Follow-up 得到的是 `[true, true]`，而不是 `[false, true]`。第一版按「哪个为 true」判断，结果永远解析成 Steer，点击看起来毫无反应。改为拿当前模式对应的向量做差分定位被点段（`next_composer_mode`），并覆盖「点已选中段得到 `[false, false]`」这一路。
2. **`Toggle` 没实现 `InteractiveElement`**，挂不上 `debug_selector`，测试无法定位分段位置。按比例猜坐标时 0.8 处正好落在分段边缘外（组宽 137px，Follow-up 右缘约在 0.82）。最终把标签包一层 `div().debug_selector(...)` 再交给 `Toggle`（它实现了 `ParentElement`），渲染结构不变但测试可精确取中心点。
3. **minimap 竖条不能占布局**。直接加 `border_l_2` 或插入竖条子元素都会让选中行比其余行宽出 2~6px，切换选中时整列抖动。改为 `relative()` + 绝对定位的 2px 竖条，只有选中项进树，未选中行排版一像素不动，同时 `debug_bounds` 天然能区分「有没有竖条」。

### 与规范的偏离

- **规范 5.4 的 thinking 展开区 `max_h` + 底部渐变遮罩 + 内部滚动未实现**。展开内容位于虚拟化 `list` 的表项内部，嵌套滚动容器会干扰 R7/R8 建立的 tail-follow 高度测量与滚动条拖拽路径（`ListState::measure_all` / `scroll_to_end`）；风险明显大于收益，本轮只落地「左竖线 + 缩进」，`max_h` 与遮罩留待后续轮次单独评估。其余条款均已落地。
- `crates/ui/src/theme.rs` 未改：gpui-component 现有 token 足够覆盖规范映射表，无需新增 token 投影（任务卡本就写的是「若规范要求且默认 token 不足」）。

### T3 目视

待用户验收：需在深/浅两种模式下各看一遍，逐项对照《UI 设计规范》确认消息区无「框套框」、工具/思考卡片弱边框 + header 状态点、侧栏操作 hover 显隐、composer 主次分明。结论由验收方回填本节。
