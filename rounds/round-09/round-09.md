# Round 09 — 前端视觉打磨（依据《UI 设计规范》）

<!-- 保存为 rounds/round-09/round-09.md；该轮其他管理产出也放在同一目录。 -->

> 执行方：**Windows** · 状态：✅ 完成 —— T1/T2/T3 全部通过（T3 于 2026-08-18 人工目视通过，含意见①处置后的复验）· PR [#16](https://github.com/cking000bigdemon/GPUI-Pi/pull/16)

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

> ⚠️ **以下自评写于规范 v1.0 时期，现已随规范演进而失效，见下方「规范升版后的落地状态」。**

- **规范 5.4 的 thinking 展开区 `max_h` + 底部渐变遮罩 + 内部滚动未实现**。展开内容位于虚拟化 `list` 的表项内部，嵌套滚动容器会干扰 R7/R8 建立的 tail-follow 高度测量与滚动条拖拽路径（`ListState::measure_all` / `scroll_to_end`）；风险明显大于收益，本轮只落地「左竖线 + 缩进」。
  - **后续结论**：该判断经 pi-web 调研证实为普遍问题而非单点取舍——基线共 9 处同类嵌套滚动容器。规范已把它升格为 **S-22 禁令**，并给出「截断 + 展开」的统一替代（保留 `max_h` 与遮罩的视觉，只去掉内部滚动）。
- `crates/ui/src/theme.rs` 未改：当时判断 gpui-component 现有 token 足够覆盖规范映射表。
  - **后续更正**：调研发现 gpui-component 默认深色主题的层级方向与 pi-web 相反（`sidebar` 与 `background` 同为 `#0a0a0a`），规范新增 **S-15** 要求覆写。该项**尚未实现**。

### 规范升版后的落地状态

T3 目视验收提出三条意见（用户消息气泡化、主区左右留白、字体比例偏大），据此规范由 v1.0 演进到 v2.1，新增 S-12 ~ S-25 共 14 条。**本轮已提交的代码只覆盖 v1.0 条款**，下表是对 v2.1 的真实状态：

| 条款 | 状态 | 说明 |
|---|---|---|
| v1.0 全部条款（消息无外层边框、工具卡状态点、左竖线从属、侧栏 hover 显隐、composer 主次分层、minimap 选中态） | ✅ 已落地 | 见上方 T1/T2 记录 |
| S-13 消息列 820 居中 + `px_4` | ✅ 已落地（第二批） | `chat.rs::message_column` 套在每个 list 表项外层；`MESSAGE_COLUMN_MAX_WIDTH = 820.` |
| S-14 用户消息右对齐气泡 | ✅ 已落地（第二批，T3 意见①后按规范 v2.2 修订） | `chat.rs::user_bubble_style` 五值纯函数（**base.blue** 0.10 底 / 0.2 边框 / 实色选中边框 / 12px 圆角 / 85% 宽），去掉 `User` 角色标签。初版误用 `accent`（实为中性 hover 色，铺出来不可见），T3 复验判红后改 `cx.theme().blue`，详见「T3 目视」 |
| S-18 正文行高 `relative(1.7)` | ✅ 已落地（第二批） | `BODY_LINE_HEIGHT = 1.7`，正文与气泡两处引用同一常量（裁决见下方「第二批备注」①） |
| S-12 正文显式 `text_sm` | ✅ 已落地（第二批） | 助手文本流与用户气泡容器均显式 `.text_sm()` |
| S-15 深色主题投影 | ✅ 已落地（第二批） | `theme.rs::apply_panel_elevation`：深色下 `sidebar`/`sidebar_border` 对齐 `title_bar`/`title_bar_border` 档（纯 token 投影，零硬编码），`colors` 与 legacy `tokens` 双写；挂进 `sync_system_theme` 随 appearance 重放 |
| S-16 dim/disabled 封装函数 | ✅ 已落地（第二批） | `theme.rs::dim_foreground`（0.7）/ `disabled_foreground`（0.5）+ 源码守卫测试；当前组件内散落用法为零（见「第二批备注」②） |
| S-25 / § 5.8 条款断言 | ✅ 已落地（第二批） | 断言明细见下方「第二批 T2」表 |
| S-20 助手消息模型名 | ⛔ 不属本轮 | 依赖 `pi-render` 增加 `Message.model`，被本轮「禁止」条排除，规范已标注生效轮次 R10 |

**结论：规范 v2.1 内属于 R9 的条款已全部落地，待 T3 人工目视。** 三个纯逻辑 crate 全程未触碰。

### 第二批实测（规范 v2.1 补齐 · 2026-08-18）

> 改动范围：`crates/ui/src/chat.rs`、`crates/ui/src/theme.rs`、`ROUNDS.md`、本任务卡。
> `pi-rpc` / `pi-data` / `pi-render` / `crates/app` 均未改动。

#### 第二批 T1 静态（与第一批同因，`validate.ps1` 缺 BOM 无法直跑，逐条执行同样五步）

| 步 | 命令 | 结果 |
|---|---|---|
| 1 | `.\scripts\check-pins.ps1` | OK（10 项） |
| 2 | `cargo fmt --all -- --check` | OK |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | OK，无 `#[allow]` |
| 4 | `cargo test --workspace` | 全绿：`gpui-pi` 31 passed、`gpui-pi-ui` **17 passed**（较第一批 +7）、`pi-render` 27 passed，其余 crate 不变；`real_pi.rs` 5 例仍 opt-in ignored |
| 5 | `cargo build --release --workspace` | OK |

#### 第二批 T2（§ 5.8 条款断言）

| 条款 | 用例 | 断言内容 |
|---|---|---|
| S-13 | `message_column_is_centered_and_capped` | 1000px 窗口列宽恰为 820 且水平居中；640px 窗口列宽 = 窗宽 − 32、起点 x=16 |
| S-14 | `user_bubble_hugs_column_right_edge` | `user-bubble` 右缘贴 `message-column` 右缘（±1px），宽度 ≤ 列宽 × 85% |
| S-14 | `user_bubble_style_matches_spec` | 纯函数四值：bg = accent(0.10)、border = accent(0.2)、radius = 12px、ratio = 0.85 |
| S-18 | `body_line_height_is_shared_constant` | 常量 = 1.7；源码级守卫：所有 `line_height` 都引用 `BODY_LINE_HEIGHT` 且 ≥2 处 |
| S-16 | `dim_and_disabled_foregrounds_are_single_source` | 两函数返回值 = muted_foreground × 0.7 / 0.5，且两档互异 |
| S-16 | `muted_foreground_opacity_is_centralized` | 扫描 `crates/ui/src` + `crates/app/src` 全部 `.rs`，除 `theme.rs` 集中定义外无手写派生 |
| S-15 | `dark_panels_are_elevated_above_canvas` | 深色下 `sidebar != background` 且亮度更高、legacy tokens 与 colors 同步；浅色不投影且方向正确 |
| 红线 10 | `only_user_messages_are_bubbled`（由 `only_user_messages_are_carded` 改名） | 仅 User 气泡化，其余角色纯文本流 |

既有用例零回归（第一批 T2 ①–⑤ 全部保留并通过）。

#### 第二批备注

1. **S-18 行高裁决**：规范 § 3.4 表把用户气泡行高写作 `relative(1.6)`，但 § 5.8 要求「消息正文与用户气泡两处都引用 `BODY_LINE_HEIGHT = 1.7`」，本轮任务指令亦明确「正文与气泡显式 `.line_height(relative(1.7))`」。按 § 5.8（可验证性条款）与任务指令取 **1.7**，规范 § 3.4 表的 1.6 建议随下次修订收敛。
2. **S-16 现状**：本仓组件内此前并无散落的 `muted_foreground` 透明度派生（grep 为零），故本条落地形式为「集中定义 + 返回值断言 + 源码守卫」；后续轮次需要 dim/disabled 档时直接消费这两个函数。
3. **顺带对齐（1 行）**：`MessageView` 的 Unknown 角色标签色由 `danger` 改为 `warning`，对齐规范 § 5.1「渲染不了 ≠ 出错了」及既有 unknown-card 的 warning 状态点；属本轮消息区视觉范围。
4. **选中态定值**：助手消息选中底色由 `accent.opacity(0.12)` 统一为 **0.16**（S-7 勘误后为定值，v2.0 前的「0.12~0.16」区间已废止）。

### T3 目视

**复验意见 ①（2026-08-18，浅色模式）**：用户气泡颜色非常不明显，要求更换成更显著的对比色。

- 根因：规范 v2.0/v2.1 的前提「本项目 accent=蓝」失实——gpui-component 默认主题的 `accent` 是
  shadcn 语义的中性 hover 色（浅 `neutral-100` / 深 `neutral-800`），`opacity(0.10)` 铺在画布上不可见。
- 处置：按「先改规范再改代码」，规范升 **v2.2**（勘误 § 2.3 / S-14 / S-24 / § 5.8，见修订记录）；
  `user_bubble_style` 基色改 `cx.theme().blue`（base.blue：浅 blue-600 / 深 blue-400，pi-web 用户蓝同族），
  新增 `selected_border` 字段；测试补「深浅两模式边框饱和度 ≥ 0.5」防回归守卫。
  处置后 T1 五步复跑全绿（测试计数不变：31 / 17 / 27）。

复验清单（深/浅两种模式各看一遍）：

- 第一批条款：消息区无「框套框」、工具/思考卡片弱边框 + header 状态点、侧栏操作 hover 显隐、composer 主次分明；
- 第二批条款：消息列 820 居中（超宽窗口留白变大、行长不变）、用户消息为右对齐弱色气泡且无 `User` 标签、正文 14px + 1.7 行高的阅读密度、**深色模式下侧栏/目录面板比画布亮一档**（切系统深浅模式验证投影重放）。

**结论（验收方回填，2026-08-18）：验收通过。** 意见①（气泡色不明显）经规范 v2.2 勘误 + `base.blue` 改色处置后复验确认。R9 就此收口，S-20 按规范标注移交 R10。
