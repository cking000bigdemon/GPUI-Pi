# GPUI-Pi UI 设计规范

> 版本：v1.0（R9 前置产出）
> 定位：本文件是 GPUI-Pi 界面视觉的唯一判据。与它冲突时以本规范为准；与立项文档冲突时以立项文档为准。
> 来源基线：
> - Zed 桌面端设计语言：`ZED_CHECKOUT/crates/theme/src/`、`ZED_CHECKOUT/crates/ui/src/styles/`、`ZED_CHECKOUT/crates/ui/src/components/`、`ZED_CHECKOUT/crates/agent_ui/src/`（下称 `ZED_*`）
> - 组件库：`GPC_CHECKOUT/crates/base/src/theme_tokens.rs`、`GPC_CHECKOUT/crates/ui/src/theme/theme_color.rs`、`GPC_CHECKOUT/crates/ui/src/button/button.rs`（下称 `GPC_*`）
> - 本项目现状：`crates/ui/src/chat.rs`、`crates/ui/src/theme.rs`、`crates/app/src/session_sidebar.rs`、`crates/app/src/panels.rs`、`crates/app/src/workspace.rs`

---

## 1. 设计原则

### 1.1 视觉层级：背景 + 留白优先，边框退居其次

Zed 的层级模型（`ZED_crates/ui/src/styles/elevation.rs`）：

| 层级 | 用法 | 背景来源 |
|---|---|---|
| Background | 应用最底层画布 | `colors().background`（= 中性色阶 step_1） |
| Surface | 面板、pane、容器主表面 | `colors().surface_background`（= step_2） |
| EditorSurface | 可编辑区（buffer / composer / 用户消息） | `colors().editor_background`（= step_1） |
| ElevatedSurface | 浮层（popover、菜单） | `colors().elevated_surface_background` + 阴影 |
| ModalSurface | 对话框、模态 | 同 elevated 背景 + 多层阴影 |

**规范 S-1**：层级一律用「背景色阶 + 留白」表达，禁止用边框堆层级。同一容器内不再套边框容器（禁止边框套边框）。
**规范 S-2**：阴影仅用于浮层（popover / tooltip / 对话框 / 消息 hover 态），日常卡片、列表行不使用阴影。
**规范 S-3**：一屏内背景层次最多用三级：`background`（画布）→ `sidebar`/`surface`（面板）→ 高亮/选中（透明度叠加），不要引入第四级。

来源：`ZED_crates/ui/src/styles/elevation.rs`（`ElevationIndex::bg`、`shadow`）、`ZED_crates/theme/src/default_colors.rs`（step 定义）

### 1.2 状态表达：小点/竖条/图标色，不铺底、不铺边

Zed 的 ColorScale 语义（`ZED_crates/theme/src/scale.rs`）：
- step_9 = 最饱和实心色，用于语义色（error/warning/success）本身的**点**、图标、文字强调；
- step_3/4/5 = 组件背景常规/hover/按下，是**中性**的（alpha 黑/白），不是语义色；
- step_6/7/8 = 边框：弱边框（非交互/交互）/强边框 + 焦点环。

**规范 S-4**：成功/警告/错误等状态只出现在：小圆点、图标色、文字色、左侧竖条，最多加一个**低透明度**背景（≤0.2）。禁止把状态色直接用作卡片整体边框色或铺满卡片背景。
**规范 S-5**：中性色（边框、背景、hover）必须走 alpha 中性色阶，不允许拿语义色降透明度冒充中性色。

来源：`ZED_crates/theme/src/scale.rs`（step_1..12 注释）、`ZED_crates/ui/src/components/callout.rs`（状态背景透明度 0.08~0.2）

### 1.3 焦点反馈：边框变色，不用 focus ring 堆色

- 常规态：`border`（step_6）；
- 可编辑/交互聚焦：`border_focused`（blue step_5），按钮等控件可用 `border_focused` + hover 时 `border_focused.opacity(0.8)`（`ZED_crates/agent_ui/src/conversation_view/thread_view.rs` 用户消息编辑态）；
- 非聚焦编辑态：`border_dashed`。

**规范 S-6**：焦点反馈 = 边框色切换，禁止用背景高饱和色、禁止加额外的描边环；gpui-component 组件自带的 focus ring 交由 `Theme::focus_ring` 全局控制，不要在组件上叠加。
**规范 S-7**：列表行 hover 用 `muted` 或 `element_hover` 透明度背景，选中用 `accent.opacity(0.12~0.16)`，选中行**不加边框**。

来源：`ZED_crates/theme/src/default_colors.rs`（border_focused/border_selected）、`ZED_crates/agent_ui/src/conversation_view/thread_view.rs:6209`（用户消息编辑态边框）

### 1.4 主次分层：三级操作可见性 + 一行不超过 3 片段

- 主操作常驻（primary 按钮或行内首个图标）；
- 次操作 hover 显隐（Zed 工具卡头的 Disclosure、消息编辑器展开按钮 `opacity(0.5) → hover 1.0`，见 `ZED_crates/agent_ui/src/conversation_view/thread_view.rs:4420`）；
- 低频操作收进菜单/右键菜单。

**规范 S-8**：一行信息展示不超过 3 个文本片段（如「标题 · 指标 · 状态」），次要信息进 tooltip。
**规范 S-9**：行内操作按钮默认只显示高频 1~2 个；低频（删除、导出、重命名）hover 显隐或收入菜单。

来源：`ZED_crates/agent_ui/src/conversation_view/thread_view.rs`（Disclosure `visible_on_hover`、subagent output 分隔）、`ZED_crates/ui/src/components/button/button.rs`

---

## 2. 语义色映射表

> 写法约定：`cx.theme().<token>` 是 gpui-component 的 `ActiveTheme`（`GPC_crates/ui/src/theme/mod.rs`），本项目一律走它，禁止硬编码颜色。
> 透明度写法：`cx.theme().accent.opacity(0.16)`。

### 2.1 中性 / 背景 / 边框

| 用途 | Zed token | 深色模式（dark step 参考值） | 浅色模式（light step 参考值） | 本项目落地 |
|---|---|---|---|---|
| 应用画布背景 | `background` = neutral step_1 | `#111110`（sand dark_1） | `#fdfdfc`（sand light_1） | `cx.theme().background` |
| 面板/侧栏/表面 | `surface_background` = step_2 | `#191918` | `#f9f9f8` | `cx.theme().sidebar`（侧栏）/ `cx.theme().popover`（浮层）/ `cx.theme().title_bar` |
| 组件常规态背景 | `element_background` = step_3 | `#222221` | `#f1f0ef` | `cx.theme().muted` |
| 组件 hover 背景 | `element_hover` = alpha step_4 | 白 alpha ≈0x1b | 黑 alpha ≈0x17 | `cx.theme().muted`（列表 hover）/ `cx.theme().secondary_hover` |
| 组件按下/选中背景 | `element_active` = alpha step_5 | 白 alpha ≈0x22 | 黑 alpha ≈0x1f | `cx.theme().secondary_active` |
| 幽灵控件 hover | `ghost_element_hover` = alpha step_3/4 | `cx.theme().muted` 同源 | 同左 | `hover(\|row\| row.bg(cx.theme().muted))` |
| 弱边框（非交互） | `border` = step_6 | `#3b3a37` | `#dad9d6` | `cx.theme().border` |
| 弱边框（交互） | `border_variant` = step_5 | `#31312e` | `#e2e1de` | `cx.theme().border`（可用 opacity 区分） |
| 焦点边框 | `border_focused` = blue step_5 | blue dark_5 | blue light_5 | `cx.theme().accent`（本项目 accent=蓝） |
| 主文本 | `text` = step_12 | `#eeeeec` | `#21201c` | `cx.theme().foreground` |
| 次要文本 | `text_muted` = step_11（深）/step_10（浅） | `#b5b3ad` | `#82827c` | `cx.theme().muted_foreground` |
| 占位文本 | `text_placeholder` = step_10 | `#7c7b74` | `#8d8d86` | `cx.theme().muted_foreground`（opacity 0.7 可选） |
| 禁用文本 | `text_disabled` = step_9 | `#6f6d66` | `#8d8d86` | `cx.theme().muted_foreground.opacity(0.5)` |
| 强调文本/链接 | `text_accent` = blue step_11 | blue dark_11 | blue light_11 | `cx.theme().accent` |
| 图标常规 | `icon` = step_11 | `#b5b3ad` | `#63635e` | `cx.theme().muted_foreground` |
| 滚动条滑块 | `scrollbar_thumb_background` = alpha step_3 | 白 alpha 0x12 | 黑 alpha 0x0f | `cx.theme().scrollbar_thumb` |

### 2.2 状态 / 语义色

| 用途 | Zed token | 来源 | 本项目落地 | 使用位置 |
|---|---|---|---|---|
| 成功 | `status().success` | `ZED_crates/ui/src/styles/color.rs` | `cx.theme().success` | 状态点、运行中图标、diff 新增行 |
| 警告 | `status().warning` | 同上 | `cx.theme().warning` | pending 状态、诊断提示、截断标记 |
| 错误 | `status().error` | 同上 | `cx.theme().danger` | 失败状态、删除确认、错误提示 |
| 信息 | `status().info` | 同上 | `cx.theme().info` | 中性提示、链接 hover |
| 提示 | `status().hint` | 同上 | `cx.theme().accent`（蓝系） | 快捷键提示、可操作提示 |
| 新增/创建 | `status().created`（绿） | 同上 | `cx.theme().success` | diff 新增、新文件 |
| 修改 | `status().modified`（黄） | 同上 | `cx.theme().warning` | diff 修改、未保存 |
| 删除/冲突 | `status().deleted` / `conflict` | 同上 | `cx.theme().danger` | diff 删除、冲突标记 |

**规范 S-10**：状态色使用时必须「点状」呈现——小圆点（`size_2` + `rounded_full`）、图标色、文本色、左竖条；若需要背景，透明度 ≤0.2（参考 Zed callout：info 0.1 / success 0.1 / warning 0.2 / error 0.08，`ZED_crates/ui/src/components/callout.rs:120`）。

### 2.3 项目特有容器映射

| 容器 | Zed 对应 | 本项目落地 |
|---|---|---|
| 用户消息底 | `editor_background`（= background） | `cx.theme().background` 或 `cx.theme().muted.opacity(0.2)`（浅一层） |
| 助手消息底 | 无（透明，纯文本流） | 不设背景 |
| 工具/思考卡片底 | `element_background` 混合白 2.5% | `cx.theme().muted.opacity(0.42)` 或 `cx.theme().popover` |
| 代码块底 | 编辑器底 | `cx.theme().muted.opacity(0.42)` |
| composer 底 | `editor_background` + 顶边框 | `cx.theme().background` + `border_t_1` |
| 对话框浮层 | `elevated_surface_background` | `cx.theme().popover` |

来源：`ZED_crates/theme/src/default_colors.rs`（全部）、`ZED_crates/agent_ui/src/conversation_view/thread_view.rs:11000`（tool_card_header_bg）

---

## 3. 间距 / 圆角 / 字号 / 字体规范

> 本项目使用 gpui 的链式便捷方法（1rem = 16px）。Zed 的 DynamicSpacing（`ZED_crates/ui/src/styles/spacing.rs`）与本项目方法的对照见下表。默认密度下两者数值一致。

### 3.1 间距刻度（默认密度）

| 语义 | Zed DynamicSpacing | 本项目方法（px） | 用途 |
|---|---|---|---|
| 最小间隙 | Base02 | `gap_0p5` = 2px | 图标与文字极紧并排 |
| 紧凑间隙 | Base04 | `gap_1` = 4px | 图标+文字、按钮内容、chip 内距 |
| 常规间隙 | Base06 | `gap_1p5` = 6px | 工具卡 header、thinking 内容左缩进 |
| 行内间隙 | Base08 | `gap_2` = 8px | 列表行、卡片内块间距、行首图标间距 |
| 段间距 | Base12 | `gap_3` = 12px | 卡片内节间距、minimap 节点组 |
| 消息流间隙 | Base16 | `gap_4` = 16px | 消息之间 |
| 区块间距 | Base20 | `gap_5` = 20px | 侧栏项目分组 |
| 大区块 | Base24 | `gap_6` = 24px | 面板内大分组 |
| 特大 | Base32/48 | `gap_8` = 32px / `px(48.)` | 空状态留白 |

来源：`ZED_crates/ui/src/styles/spacing.rs`（derive_dynamic_spacing 列表：Base04=4、Base06=6、Base08=8、Base12=12、Base16=16、Base20=20）

### 3.2 各场景节奏（消息流 / 列表行 / 卡片 / composer）

| 场景 | 外边距 | 内边距 | 圆角 | 说明 |
|---|---|---|---|---|
| 助手消息条目 | `px_5`（20px 左右）、`py_1p5`（6px）、末条 `pb_4` | 无 | 无 | 纯文本流，无框无底（Zed 助手消息，`thread_view.rs:6395`） |
| 用户消息条目 | `px_2`、上下 `pt_2 pb_3` | `px_2 py_2`（编辑态 `py_3 px_2`） | `rounded_md`(6px) | `bg(editor_background)` + `border_1`，不透明窗口加 `shadow_md`（`thread_view.rs:6203`） |
| 缩进消息（子代理/处理详情） | 外层 `pl_5`（20px） | 无 | 无 | 左竖线 `w_px` 位于 18px 处，背景 `panel_background.opacity(0.2)`（`thread_view.rs:6511`） |
| 列表行（侧栏会话） | 无 | `px_2 py_1`~`p_2` | `rounded_md` | hover `muted`、选中 `accent.opacity(0.12~0.16)`，行间 `gap_1`（`session_sidebar.rs`） |
| 工具卡片 | 消息流内 `gap_2` | `p_2` | `rounded_md` | 弱边框 `border`（见 5.3） |
| thinking 卡片 | 消息流内 `gap_2` | header 无、内容 `pl_3p5`(14px) | 无边框卡，左竖线 | header 高 `line_height - 2px`（`thread_view.rs:7450`） |
| 代码块 | 无 | 内容 `p_2`、header `px_2 py_1` | `rounded_md` | header 下 `border_b_1` |
| composer | 底部固定 | 容器 `px_2 py_2` | 无 | 顶边框 `border_t_1`，`bg(editor_background)`（`thread_view.rs:4353`） |
| minimap/目录节点 | — | `px_2 py_1` | `rounded_sm` | 缩进 `ml(level * 7px)`，选中 `accent.opacity(0.16)`（`chat.rs` 现状） |

### 3.3 圆角刻度

| 本项目方法 | 像素 | Zed 对应 | 用途 |
|---|---|---|---|
| `rounded_xs` | 2px | — | 小状态点以外的极小元素 |
| `rounded_sm` | 4px | 按钮圆角（`button_like.rs:792` rounded_sm） | 按钮、minimap 节点、小 chip |
| `rounded_md` | 6px | 用户消息、卡片 | 消息条目、工具卡、代码块、列表行 |
| `rounded_lg` | 8px | Callout、面板 | 对话框内卡片 |
| `rounded_xl` | 12px | — | 对话框（gpui-component `radius_lg` 默认 12px） |
| `rounded_full` | 9999px | — | 状态点、头像、圆钮 |

来源：`ZED_crates/gpui_macros/src/styles.rs`（corner_suffixes：xs=0.125rem、sm=0.25rem、md=0.375rem、lg=0.5rem、xl=0.75rem）

**规范 S-11**：圆角只用上表 6 个刻度，禁止自定义中间值；相邻嵌套元素圆角保持「同心收缩」（如卡片 `rounded_lg` 内嵌 header 用 `rounded_md`）。

### 3.4 字号 / 字重 / 行高

| 用途 | Zed 语义字号 | 本项目 div 方法 | 像素 | 行高参考 | 字重 |
|---|---|---|---|---|---|
| 最小辅助文本 | TextSize::XSmall | `text_xs` | 12px（Zed 的 XSmall 为 10px，本项目统一 12px 为最小） | 16px | normal |
| 次文本 / 元信息 | TextSize::Small | `text_xs` | 12px | 16px | normal |
| 常规 UI 文本 | TextSize::Default（14px） | `text_sm` | 14px | 20px | normal |
| 消息正文 | 同常规 | `text_sm`（正文）/ `text_base` 用于长正文可读性 | 14~16px | 20~24px | normal |
| 强调标题 | Headline::Small（16px） | `text_base` + `font_semibold` | 16px | 24px | semibold |
| 面板标题 | Headline::Medium（18px） | `text_lg` + `font_semibold` | 18px | 28px | semibold |
| 大标题 | Headline::Large/XLarge | `text_xl`+（20px） | 20px | 28px | semibold |

- 字重：正文 normal；标题 semibold；**禁止**在 14px 以下使用 bold（Zed 只用 `font_semibold` 做标题，见 `ZED_crates/ui/src/styles/typography.rs` Headline 实现）。
- 等宽字体：`font_family(cx.theme().mono_font_family.clone())` 用于代码、工具输入输出 JSON、diff；等宽字号 13px 为佳（gpui-component mono_md = 13/20，`GPC_crates/base/src/theme_tokens.rs`）。
- 工具名/卡片标题字号：13px（Zed `tool_name_font_size` = rems_from_px(13)，`thread_view.rs:11008`），本项目用 `text_sm`（14px）或 `text_xs`（12px）就近取整，**全应用统一**，不要混用。

来源：`ZED_crates/ui/src/styles/typography.rs`（TextSize/HeadlineSize）、`ZED_crates/gpui/src/styled.rs`（text_xs=0.75rem 等）、`GPC_crates/base/src/theme_tokens.rs`（TypographyTokens）

### 3.5 字体

| 场景 | 字体 | 落地 |
|---|---|---|
| UI 字体 | Windows：微软雅黑 UI；其余：Noto Sans SC | `crates/ui/src/theme.rs` 已实现（`UI_FONT_FAMILY`），禁止在组件里另设 |
| 等宽字体 | Windows：Consolas；其余：Noto Sans Mono CJK SC | `MONO_FONT_FAMILY`（`crates/ui/src/theme.rs`） |
| 内嵌 CJK 兜底 | Noto Sans SC（离线资源） | `init_fonts` 已注册 |

来源：`crates/ui/src/theme.rs`、`GPC_crates/base/src/theme_tokens.rs`（default_mono_font_family）

---

## 4. 组件使用规范

### 4.1 按钮分级

| 级别 | gpui-component API | 用法 | 对应 Zed |
|---|---|---|---|
| 主操作 | `Button::new(id).primary().label("…")` | 每面板最多 1 个常驻主按钮（composer 发送、重命名确认） | Filled + Tinted(Accent) |
| 次操作 | `.secondary()` / `.default()` | 对话框确认、行内保存 | Filled（中性） |
| 幽灵操作 | `.ghost()` | 工具栏图标、行内操作、minimap 开关 | Transparent（ghost） |
| 危险操作 | `.danger()` | 删除确认、破坏性操作 | Tinted(Error) |
| 文本按钮 | `.text()` | 链接式操作 | 无边框按钮 |

- 尺寸：工具栏 `small`（h=28 对应 Zed Medium 28px）；行内/折叠操作 `xsmall`；主按钮 `medium`。
- 图标按钮必须带 `.tooltip("…")`；tooltip 文案动词开头（「刷新」「隐藏目录」）。
- 禁用态 `.disabled(busy)`；运行中禁止重复触发（本项目 `busy_actions` 模式）。
- 按钮内图标与文字间距 `gap_1`（4px）；纯图标按钮给 `gap_0` 并保证可点区域 ≥ 22px 高（Zed ButtonSize::Default = 22px，`button_like.rs:470`）。

来源：`GPC_crates/ui/src/button/button.rs`（ButtonVariant）、`ZED_crates/ui/src/components/button/button_like.rs`（ButtonSize、ButtonStyle）

### 4.2 卡片（弱边框卡）

- 结构：`v_flex().p_2().rounded_md().border_1().border_color(cx.theme().border)`；
- 卡内第一段是 header：`h_flex().gap_1p5()`，header 与内容之间用 `gap_2` 或 `border_t_1`（内容多块时）；
- **禁止**：卡片内再套完整卡片（工具输出、diff 只能以左竖线/文本块下沉）。

### 4.3 列表行

- 结构：`div().px_2().py_1().rounded_md().cursor_pointer()`；
- hover：`.hover(|row| row.bg(cx.theme().muted))`；
- 选中：`.bg(cx.theme().accent.opacity(0.16))`，不加边框；
- 行内信息最多 3 片段：标题（`text_sm` 截断）+ 指标行（`text_xs` + `muted_foreground`）+ 状态图标色；
- 缩进（树形会话）：`ml(depth * 12px)`，与 minimap 缩进刻度（7px/级）不同用途，列表用 12px。

来源：`crates/app/src/session_sidebar.rs`（现状）、`ZED_crates/ui/src/components/list/`（行 hover 语义）

### 4.4 折叠（Disclosure / 手风琴）

- 折叠头：`h_flex().gap_1p5().cursor_pointer()`，图标 `ChevronRight/ChevronDown`（`IconName`），标题 `text_sm`；
- 折叠头 hover 反馈：`hover(|row| row.bg(cx.theme().muted))` 或仅变色；
- 展开/收起指示文字（「展开/收起」）是次信息，可用 `text_xs + muted_foreground`，或学 Zed 用 `visible_on_hover` 只在 hover 显示 chevron（`thread_view.rs:7471` Disclosure + `visible_on_hover`）；
- 内容区：展开时保持左对齐，与折叠头同宽；长内容走左竖线模式（见 5.3）。

### 4.5 状态点 / 状态竖条

- 状态点：`div().size_2().rounded_full().bg(cx.theme().<status>)`（8px 圆点）；
- 竖条（工具输出从属关系）：`div().w_px().bg(cx.theme().border)`，位于容器左缘 18px 处（Zed 缩进竖线，`thread_view.rs:6516`）；
- 状态点旁文字用 `text_xs + muted_foreground`，**状态色只上点/图标，不上文字**（或文字用 `text_xs` 同色，与点并列时不重复）。

### 4.6 Tooltip / Popover / 右键菜单

- Tooltip：一律组件内建 `.tooltip("…")`；触发延迟与样式走组件库默认，不自定义；
- Popover：浮层背景 `cx.theme().popover`、`rounded_md`、`border_1 + border`、阴影走组件库；浮层内列表项 hover 用 `secondary_hover`；
- 低频操作（删除/导出/更多）收右键菜单或「更多」popover，行内只留高频操作；
- 对话框：`window.open_dialog`，背景 `popover`，主按钮 `primary`、取消 `secondary`、危险 `danger`（`ok_variant(ButtonVariant::Danger)`，见 `session_sidebar.rs` 删除确认）。

### 4.7 Divider

- 水平分隔：`Divider::horizontal()`（Zed 组件）或 `div().h_px().w_full().bg(cx.theme().border.opacity(0.6))`；
- 垂直分隔：`w_px()`，用于工具栏分组、属性键值；
- 分隔线不用于「套层级」，只用于同层分组；卡片与卡片之间用留白（`gap_3`+）而非分隔线。

来源：`ZED_crates/ui/src/components/divider.rs`、`ZED_crates/agent_ui/src/conversation_view/thread_view.rs:6471`（Subagent Output 分隔线用法）

---

## 5. AI 面板设计模式（消息流 / 工具卡 / thinking / minimap）

### 5.1 消息条目

**用户消息**（参考 `thread_view.rs:6160` 用户消息 + `chat.rs` 现状）：
```
div().p_2().rounded_md().border_1().border_color(cx.theme().border)
    .bg(cx.theme().background)                 // 或 muted.opacity(0.2) 浅一层
    .hover(|row| row.border_color(cx.theme().accent.opacity(0.8)))  // 可编辑时
```
- 用户消息底色与画布同源（editor_background），靠边框与圆角识别，不铺 accent 色；
- 头部行：角色标签（`text_xs + font_semibold`）+ 标签（`text_xs + muted_foreground`），两者 `gap_2`；
- 角色色：User = `accent_foreground`（或保持中性 `foreground`）；Assistant = `foreground`；Compaction/BranchSummary = `warning`；Unknown = `danger`。

**助手消息**（参考 `thread_view.rs:6395`）：
```
v_flex().w_full().px_5().py_1p5().gap_2()      // 无背景无边框
```
- 助手消息是纯文本流，不加卡片；正文 `text_sm`（14px）+ 行高 20px，长文可 `text_base`；
- 首条消息顶部留 `pt_2`，末条底部 `pb_4`。

**处理详情/子代理输出**（参考 Zed 缩进消息 `thread_view.rs:6506`）：
- 外层 `pl_5` + 左竖线（18px 处 `w_px`）+ `bg(panel_background.opacity(0.2))`；
- 顶部用水平分隔线 + 小图标 + 标签（如「处理详情 · N 条消息 · M 次工具调用」`text_xs + muted_foreground`）。

### 5.2 工具调用卡片

结构（参考 `thread_view.rs:8159` render_tool_call + `chat.rs` render_tool 现状）：

```
v_flex().p_2().rounded_md().border_1()
    .border_color(cx.theme().border.opacity(0.8))   // 统一弱边框，不用状态色
    // header
    .child(h_flex().gap_1p5().cursor_pointer()
        .child(chevron)                             // ChevronRight/Down
        .child(icon 或小圆点: size_2 rounded_full bg(status))
        .child(div().text_sm().font_semibold().child(工具名))
        .child(div().flex_1())
        .child(div().text_xs().muted_foreground().child(状态文字)))  // pending/success/error
    // 摘要行
    .child(div().text_xs().truncate().muted_foreground().child(preview))
    // 展开区
    .when(expanded, |v| v.child(展开内容))
```

- 边框：一律 `border.opacity(0.8)`（Zed `tool_card_border_color`，`thread_view.rs:11004`）；
- header 底色：不加或 `muted.opacity(0.25)`（Zed `tool_card_header_bg` 是 element_background 混 2.5% 前景色，`thread_view.rs:10997`）；
- 状态：**小圆点或图标色**（pending=warning / success=success / error=danger / empty=muted_foreground）+ 状态文字用 `muted_foreground`；禁止状态色铺边框（现状 `border_color(color.opacity(0.7))` 必须改）；
- 展开区多块之间 `border_t_1` 分隔；header 行高与文本行高一致（约 20px），`gap_1p5`。

### 5.3 工具输出：左竖线 + 缩进（不嵌套卡片）

参考 `thread_view.rs:10408`（render_markdown_output 非卡片布局）与 `chat.rs` 现状：

```
v_flex().ml_1p5()          // 6px 左缩进
    .pl_3p5()              // 14px 内边距
    .border_l_1()          // 左竖线
    .border_color(cx.theme().border.opacity(0.8))
    .gap_2()
    .text_xs().text_color(cx.theme().muted_foreground)
```
- 工具输出、thinking 内容、diff 展开均用此「左竖线 + 缩进」表达从属关系，**禁止再套一张卡片**；
- 输出内小块间用 `gap_2`；Input JSON、输出文本用等宽字体（mono 13px）；
- 多段输出（Text/Ansi/Image/Diff）同竖线区内顺序排列，`gap_2`。

### 5.4 Thinking 折叠

参考 `thread_view.rs:7434`（render_thinking_block）：

- header：`h_flex().gap_1p5().cursor_pointer()`，图标 `IconName::ToolThink`（或灯泡）小尺寸 + `muted_foreground`，标题「思考」`text_sm + font_semibold`；
- chevron 右侧，hover 才显隐（`visible_on_hover` 语义，本项目可用 `hover` 控制 opacity）；
- 展开内容：左竖线模式（同 5.3），`max_h` 限制（如 `max_h(px(256.))`）+ 底部渐变遮罩 + 内部滚动（Zed `max_h_64` + 渐变 `panel_bg.opacity(0.8)→0`，`thread_view.rs:7527`）；
- thinking 卡不套边框（它是消息流内的次级内容），用竖线区分层级；若保留弱边框则必须用 `border`，不得用强调色。

### 5.5 Minimap / 目录

- 面板：`w(px(176.))`、`border_l_1 + border`、`bg(sidebar)`（`chat.rs` 现状已符合）；
- 节点：`px_2 py_1 rounded_sm text_xs truncate`，缩进 `ml((level-1) * 7px)`，`gap_1`；
- 选中：`bg(accent.opacity(0.16))`；hover：`bg(muted)`；点击滚动到消息并保持焦点在消息列表（`chat.rs` 现状已符合）；
- 目录面板顶部：关闭按钮 `ghost + xsmall + icon`，`justify_end + p_1`（现状已符合）。

### 5.6 Composer

参考 `thread_view.rs:4333`（render_message_editor）与 `panels.rs` 现状：

- 容器：`border_t_1 + border`、`bg(background)`、内容 `px_2 py_2`；
- 输入区高度：常态 76px（现状），聚焦/多行自动增高，上限约 60vh；
- 输入区下方工具行：左 = 附件/上下文控件，右 = 模式切换（steer/follow-up）+ 发送主按钮（`primary`）；工具行控件用 `ghost + small`；
- 发送按钮常驻主操作；运行中显示 loading 并禁用；
- 输入框 placeholder：`muted_foreground`；附件 chip：`rounded_md + border_1 + border`。

---

## 6. 禁止事项（红线）

1. **禁止硬编码颜色**：一切颜色走 `cx.theme()` token；特殊色（如 diff 语义色）只能以 token + `opacity()` 派生。唯一例外：ANSI 16 色映射已在 `chat.rs::ansi_color` 统一处理，新增时必须在同一函数内扩展。
2. **禁止边框套边框**：卡片内不允许再出现完整描边卡片；工具输出/thinking/diff 一律走左竖线 + 缩进。
3. **禁止状态色铺满**：状态色不得作为卡片整体边框色（如 `border_color(color.opacity(0.7))`）、不得铺满卡片背景；只能点/竖条/图标/低透明度背景（≤0.2）。
4. **禁止自造字号/圆角/间距**：只用第 3 节的刻度；`px(n)` 自定义值仅限特殊场景（minimap 宽 176、消息流 max_h 等）并需注明原因。
5. **禁止 UI 字体混用**：正文/标题一律走 `theme.font_family`（微软雅黑 UI / Noto Sans SC），不得在组件内指定其他非等宽字体；等宽只能走 `mono_font_family`。
6. **禁止在 14px 以下用 bold**；标题字重只用 `font_semibold`。
7. **禁止行内超 3 片段**信息堆叠；次要信息进 tooltip。
8. **禁止无 tooltip 的纯图标按钮**（工具栏除外且需全局有 aria-label）。
9. **禁止无状态反馈的 hover 空白**：可交互元素必须给 hover 背景（`muted` / `secondary_hover`）或边框色变化。
10. **禁止在 AI 消息流里给助手消息加卡片背景**（保留纯文本流），用户消息卡片化仅限 `rounded_md + border_1`。

---

## 7. 来源文件索引（复核用）

| 主题 | 文件 |
|---|---|
| ColorScale 12 步语义 | `ZED_crates/theme/src/scale.rs` |
| 默认主题语义色全表（深/浅） | `ZED_crates/theme/src/default_colors.rs` |
| UI 密度 / DynamicSpacing | `ZED_crates/theme/src/ui_density.rs`、`ZED_crates/ui/src/styles/spacing.rs` |
| 字号体系（TextSize/Headline） | `ZED_crates/ui/src/styles/typography.rs` |
| 层级与阴影（ElevationIndex） | `ZED_crates/ui/src/styles/elevation.rs` |
| 语义色枚举（Color/status） | `ZED_crates/ui/src/styles/color.rs` |
| 动画时长（50/150/300ms） | `ZED_crates/ui/src/styles/animation.rs` |
| 按钮样式/尺寸 | `ZED_crates/ui/src/components/button/button_like.rs`、`button/button.rs` |
| Callout（状态低透明背景） | `ZED_crates/ui/src/components/callout.rs` |
| 用户消息/缩进/竖线 | `ZED_crates/agent_ui/src/conversation_view/thread_view.rs:6119`、`6506` |
| 工具卡片/header/输出竖线 | 同上 `:8159`、`9961`、`10179`、`10408`、`10997` |
| thinking 折叠 | 同上 `:7434` |
| composer | 同上 `:4333` |
| 组件库语义 token（radius/spacing/typography/shadow） | `GPC_crates/base/src/theme_tokens.rs` |
| 组件库主题色面 | `GPC_crates/ui/src/theme/theme_color.rs` |
| 组件库按钮变体 | `GPC_crates/ui/src/button/button.rs` |
| gpui 便捷方法像素值（rounded/gap/text） | `ZED_crates/gpui_macros/src/styles.rs`、`ZED_crates/gpui/src/styled.rs` |
| 本项目主题初始化/字体 | `crates/ui/src/theme.rs` |
| 本项目消息/工具/thinking/minimap | `crates/ui/src/chat.rs` |
| 本项目会话侧栏 | `crates/app/src/session_sidebar.rs` |
| 本项目 composer/面板 | `crates/app/src/panels.rs` |
| 本项目窗口/工具栏/dock | `crates/app/src/workspace.rs` |

> 路径前缀说明：`ZED_CHECKOUT` = `C:/Users/ZhuanZ（无密码）/.cargo/git/checkouts/zed-a70e2ad075855582/bc538de/`；`GPC_CHECKOUT` = `C:/Users/ZhuanZ（无密码）/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/000114a/`。以上均为只读调研路径，本规范不复制任何 Zed 代码，仅提炼设计语言。
