# Round 09 — 前端视觉打磨（依据《UI 设计规范》）

<!-- 保存为 rounds/round-09/round-09.md；该轮其他管理产出也放在同一目录。 -->

> 执行方：**Windows** · 状态：⬜ 未开始

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

<!-- 完成后回填：实际数字、踩的坑、与设计的偏离及原因 -->
