# GPUI-Pi UI 设计规范

> 版本：**v2.3**（修订历史见 [`UI设计规范-CHANGELOG.md`](UI设计规范-CHANGELOG.md)）
> 定位：本文件是 GPUI-Pi 界面视觉的唯一判据。与它冲突时以本规范为准；与立项文档冲突时以立项文档为准。
> **单一定义原则（v2.3 起）**：每个数值在全文只有**一个**定义点，其余章节只许写指针（「见 § x.y」/「见 S-n」），
> 不许复述数值。v2.1 与 v2.2 的勘误之所以各漏改 1~4 处，全部是因为同一数值有 3~5 份副本。
> 新增条款时若发现要在第二处写下同一个数字，说明该数字的定义点没找对。
> 来源基线：
> - Zed 桌面端设计语言：`ZED_CHECKOUT/crates/theme/src/`、`ZED_CHECKOUT/crates/ui/src/styles/`、`ZED_CHECKOUT/crates/ui/src/components/`、`ZED_CHECKOUT/crates/agent_ui/src/`（下称 `ZED_*`）
> - 组件库：`GPC_CHECKOUT/crates/base/src/theme_tokens.rs`、`GPC_CHECKOUT/crates/ui/src/theme/theme_color.rs`、`GPC_CHECKOUT/crates/ui/src/button/button.rs`（下称 `GPC_*`）
> - **功能与阅读体验对照基线 pi-web 0.8.9**：`vendor/upstream/pi-web-0.8.9/`（下称 `PIWEB_*`，钉死于 `v0.8.9` / `2a6e5371`）。Zed 定义的是「工具型 IDE 面板」的语言，pi-web 定义的是「对话阅读」的语言；**消息流内部（消息列宽度、用户消息形态、正文字号）以 pi-web 为准，其余（层级、状态表达、控件分级、卡片结构）仍以 Zed 为准**。两者冲突时按此分工裁决。
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
**规范 S-2**：阴影仅用于浮层（popover / tooltip / 对话框），**外加 composer 输入壳一处豁免**（`shadow_sm`——输入焦点区需要从画布浮起，这是全应用唯一的非浮层阴影）。日常卡片、列表行、消息条目一律不使用阴影。
**规范 S-3**：**表面层级**最多三级——`background`（画布）→ `sidebar`（面板）→ `popover`（浮层与对话框），不要引入第四级表面。

对话框与 popover **共用底色**，靠 `cx.theme().overlay` 遮罩（而非第四个底色）与下层分离。

「表面」指的是**独立成块、有自己边界的容器底色**。以下**不计入**层级数，可自由叠加：

- **交互态**：hover（`muted`）、选中（`accent.opacity(0.16)`）、按下（`secondary_active`）——它们是同一表面上的临时叠加，不是新层；
- **组件底**：工具卡、代码块等的 `muted.opacity(x)`；
- **深浅两套的绝对亮度**：S-15 说的「深色下面板比画布亮」是**方向**规定，不增加层级数。

这样计数才可判：一屏内出现第四个「独立容器底色」才算违规。

**规范 S-15（深色模式的层级方向）**：深色下**面板必须比画布亮**，与浅色下的方向相反。

pi-web 的四档背景是 `--bg #1a1a1a` → `--bg-panel #242424` → `--bg-hover #2e2e2e` → `--bg-selected #383838`（`PIWEB_app/globals.css:39-50`），逐级**变亮**；而 gpui-component 默认深色主题里 `sidebar` 与 `background` 同为 `#0a0a0a`，只靠 `title_bar #171717` 提亮。**吃默认值会丢掉「面板浮在画布之上」的观感**，这是深色模式下最主要的观感差距来源。

处置：在 `crates/ui/src/theme.rs` 里集中覆写深色的 `sidebar` / `sidebar_border` / `title_bar` / `status_bar` / `tab_bar`，让它们比 `background` 亮一档。**只允许在该文件做这层投影**，组件内一律仍走 `cx.theme()` token（红线 1）。

来源：`ZED_crates/ui/src/styles/elevation.rs`（`ElevationIndex::bg`、`shadow`）、`ZED_crates/theme/src/default_colors.rs`（step 定义）、`PIWEB_app/globals.css:21-53`、`GPC_crates/ui/src/theme/theme_color.rs:229-299`

### 1.2 状态表达：小点/竖条/图标色，不铺底、不铺边

Zed 的 ColorScale 语义（`ZED_crates/theme/src/scale.rs`）：
- step_9 = 最饱和实心色，用于语义色（error/warning/success）本身的**点**、图标、文字强调；
- step_3/4/5 = 组件背景常规/hover/按下，是**中性**的（alpha 黑/白），不是语义色；
- step_6/7/8 = 边框：弱边框（非交互/交互）/强边框 + 焦点环。

**规范 S-4**：状态色（**仅指 `success` / `warning` / `danger` / `info` 四色**，见 S-24）只出现在：小圆点、图标色、文字色、左侧竖条，最多加一个**低透明度**背景（≤0.2）。禁止把状态色直接用作卡片整体边框色或铺满卡片背景。
**规范 S-5**：中性色（边框、背景、hover）必须走 alpha 中性色阶，不允许拿上述四个**状态色**降透明度冒充中性色。（`accent` 不受此条约束，见 S-24。）

**规范 S-16（三级文本，dim 级）**：文本只有三级，且第三级必须统一派生，不许各处自行 `opacity`。

pi-web 有三级文本色（`--text` / `--text-muted` / `--text-dim`），其中最弱的 `--text-dim` 被引用 **216 次**——时间戳、行号、模型名、耗时全在这一级。gpui-component 只有 `foreground` + `muted_foreground` 两级，**没有第三级 token**。

本项目文本共四档，**全部必须走封装函数，禁止在组件里各写各的 `opacity`**：

| 档 | 落地 | 封装 | 用途 |
|---|---|---|---|
| 1 主文本 | `cx.theme().foreground` | 直接用 | 正文、标题 |
| 2 次文本 | `cx.theme().muted_foreground` | 直接用 | 元信息、placeholder、图标 |
| 3 弱文本（dim） | `muted_foreground.opacity(0.7)` | **`dim_foreground(cx)`** | 时间戳、行号、模型名、耗时 |
| 4 禁用 | `muted_foreground.opacity(0.5)` | **`disabled_foreground(cx)`** | 仅禁用态 |

两个封装函数定义在 `crates/ui` 内，与 S-15 的主题投影同属「只在 `crates/ui` 集中定义」的范畴。第 3、4 档之外**不允许再造第五档**，也不允许对 `muted_foreground` 使用这两个值以外的 `opacity`。

来源：`ZED_crates/theme/src/scale.rs`（step_1..12 注释）、`ZED_crates/ui/src/components/callout.rs`（状态背景透明度 0.08~0.2）

### 1.3 焦点反馈：边框变色，不用 focus ring 堆色

- 常规态：`border`（step_6）；
- 可编辑/交互聚焦：`border_focused`（blue step_5），按钮等控件可用 `border_focused` + hover 时 `border_focused.opacity(0.8)`（`ZED_crates/agent_ui/src/conversation_view/thread_view.rs` 用户消息编辑态）；
- 非聚焦编辑态：`border_dashed`。

**规范 S-6**：焦点反馈 = 边框色切换，禁止用背景高饱和色、禁止加额外的描边环；gpui-component 组件自带的 focus ring 交由 `Theme::focus_ring` 全局控制，不要在组件上叠加。
**规范 S-7**：列表行 hover 用 `cx.theme().muted`（或组件自带的 `list_hover`），选中用 `cx.theme().accent.opacity(0.16)`，选中行**不加边框**。

> 0.16 是定值，不是区间。v2.0 及更早写作「0.12~0.16」，已产生两个实际值，不再允许。

来源：`ZED_crates/theme/src/default_colors.rs`（border_focused/border_selected）、`ZED_crates/agent_ui/src/conversation_view/thread_view.rs:6209`（用户消息编辑态边框）

### 1.4 主次分层：三级操作可见性 + 一行不超过 3 片段

- 主操作常驻（primary 按钮或行内首个图标）；
- 次操作 hover 显隐（Zed 工具卡头的 Disclosure、消息编辑器展开按钮 `opacity(0.5) → hover 1.0`，见 `ZED_crates/agent_ui/src/conversation_view/thread_view.rs:4420`）；
- 低频操作收进菜单/右键菜单。

**规范 S-8**：一行信息展示不超过 3 个**文本片段**，次要信息进 tooltip。

「片段」的判定规则（否则无法判红）：**一个独立的文本节点算一个片段**；`·` 分隔的每一段各算一个；**图标、状态点、按钮不计入**；被 tooltip 承载的内容不计入。这样用 `debug_selector` 数子节点即可断言。
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
| 焦点边框 | `border_focused` = blue step_5 | blue dark_5 | blue light_5 | `cx.theme().accent`（**中性色，见下方待决项**） |
| 主文本 | `text` = step_12 | `#eeeeec` | `#21201c` | `cx.theme().foreground` |
| 次要文本 | `text_muted` = step_11（深）/step_10（浅） | `#b5b3ad` | `#82827c` | `cx.theme().muted_foreground` |
| 占位文本 | `text_placeholder` = step_10 | `#7c7b74` | `#8d8d86` | `cx.theme().muted_foreground`（定值，不加 opacity） |
| **第三级弱文本（dim）** | —（pi-web `--text-dim`） | `#9ca3af` | `#6b7280` | **`cx.theme().muted_foreground.opacity(0.7)`**，见 S-16 |
| 禁用文本（第四级，唯一额外档） | `text_disabled` = step_9 | `#6f6d66` | `#8d8d86` | `cx.theme().muted_foreground.opacity(0.5)`，同样封装成 `disabled_foreground(cx)`，见 S-16 |
| 强调文本/链接 | `text_accent` = blue step_11 | blue dark_11 | blue light_11 | **`cx.theme().link`**（不要用 `accent`，见下方待决项） |
| 图标常规 | `icon` = step_11 | `#b5b3ad` | `#63635e` | `cx.theme().muted_foreground` |
| 滚动条滑块 | `scrollbar_thumb_background` = alpha step_3 | 白 alpha 0x12 | 黑 alpha 0x0f | `cx.theme().scrollbar_thumb` |

> **待决项（v2.3 登记）**：Zed 的 `border_focused` / `text_accent` 都是**蓝**（左侧三列如实记录），但右侧把它们映射到
> `cx.theme().accent` —— 而 gpui-component 的 `accent` 是 shadcn 语义的**中性 hover 色**，与 v2.2 判红用户气泡时
> 是同一个失实前提。链接色本轮改判 `cx.theme().link`；**焦点边框维持 `accent` 不动**（改它会连带 S-6 与全局
> `Theme::focus_ring`，属实现改动，须由承接轮次任务卡授权）。在那之前，**S-6 在浅色模式下不作为判红依据**。

### 2.2 状态 / 语义色

| 用途 | Zed token | 来源 | 本项目落地 | 使用位置 |
|---|---|---|---|---|
| 成功 | `status().success` | `ZED_crates/ui/src/styles/color.rs` | `cx.theme().success` | 状态点、运行中图标、diff 新增行 |
| 警告 | `status().warning` | 同上 | `cx.theme().warning` | pending 状态、诊断提示、截断标记 |
| 错误 | `status().error` | 同上 | `cx.theme().danger` | 失败状态、删除确认、错误提示 |
| 信息 | `status().info` | 同上 | `cx.theme().info` | 中性提示、链接 hover |
| 新增/创建 | `status().created`（绿） | 同上 | `cx.theme().success` | diff 新增、新文件 |
| 修改 | `status().modified`（黄） | 同上 | `cx.theme().warning` | diff 修改、未保存 |
| 删除/冲突 | `status().deleted` / `conflict` | 同上 | `cx.theme().danger` | diff 删除、冲突标记 |

**状态色只有以上四色**：`success` / `warning` / `danger` / `info`。

**规范 S-10**：状态色使用时必须「点状」呈现——小圆点（`size_2` + `rounded_full`）、图标色、文本色、左竖条；若需要背景，透明度 ≤0.2（参考 Zed callout：info 0.1 / success 0.1 / warning 0.2 / error 0.08，`ZED_crates/ui/src/components/callout.rs:120`）。

**规范 S-24（`accent` 是强调色，不是状态色）**：`accent` **不属于**状态色，S-4 / S-5 / S-10 与红线 3 都**不约束**它。

这条必须单列，因为 `accent` 在本规范里同时承担多个角色，而状态色的「只点不铺」禁令会与其中几个直接打架：

| 角色 | 用法 | 定义处 |
|---|---|---|
| 身份标识 | ~~用户消息气泡底~~（v2.2 起气泡改由 `blue` 承担，见 S-14/§ 2.3；`blue` 同样不属状态色，本条豁免同样适用） | S-14 |
| 选中态 | 列表行、minimap 节点底 `opacity(0.16)` | S-7 |
| 焦点态 | `border_focused` 边框色 | S-6 |
| 链接 / 可操作提示 | 文字色（或用 `cx.theme().link`） | § 2.1 |

**为避免焦点态与选中态在同一控件上无法区分**：选中用**背景**（`opacity(0.16)`），焦点用**边框**（不加背景）。两者同时出现时，读者看到的是「有色底 + 有色框」，语义仍可分辨。

> v2.0 及更早把 `accent` 登记为 `status().hint`，导致 S-4/红线 3 与 S-14/S-7 互相矛盾。此条即为修正。

### 2.3 项目特有容器映射

| 容器 | Zed 对应 | 本项目落地 |
|---|---|---|
| 用户消息气泡底 | —（改用 pi-web 基线，见 5.1） | **`cx.theme().blue.opacity(0.10)`；边框 `cx.theme().blue.opacity(0.2)`**（v2.2 勘误：`accent` 在 gpui-component 里是中性 hover 色，铺不出可见的身份色）。`blue` 即 base.blue（浅 blue-600 / 深 blue-400），与 pi-web `--user-bg`（浅 `#eff6ff` / 深 `#1e293b`）、`border: 1px solid rgba(59,130,246,0.2)` 同族 |
| 助手消息底 | 无（透明，纯文本流） | 不设背景 |
| 工具/思考卡片底 | `element_background` 混合白 2.5% | **不铺底**（靠 `border.opacity(0.8)` 弱边框区分），header 同样不铺底 |
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
| **中密度间隙** | —（pi-web 高频档） | **`gap_2p5` / `p_2p5` = 10px** | 见 S-17；gpui 原生刻度（`rems(0.625)`），不是自定义值 |
| 段间距 | Base12 | `gap_3` = 12px | 卡片内节间距、minimap 节点组 |
| 消息流间隙 | Base16 | `gap_4` = 16px | 消息之间 |
| 区块间距 | Base20 | `gap_5` = 20px | 侧栏项目分组 |
| 大区块 | Base24 | `gap_6` = 24px | 面板内大分组 |
| 特大 | Base32/48 | `gap_8` = 32px / `px(48.)` | 空状态留白 |

来源：`ZED_crates/ui/src/styles/spacing.rs`（derive_dynamic_spacing 列表：Base04=4、Base06=6、Base08=8、Base12=12、Base16=16、Base20=20）

**规范 S-17（从 pi-web 迁移数值时向刻度取整）**：pi-web 的间距值域是 1–32px 的**连续整数**（实测 19 个不同值），其中 `3 / 5 / 7 / 9 / 11 / 18` 六个奇值合计约占 28%——它们是累积出来的，不构成设计意图（同类 chip 上 5 和 6 混用、同类按钮上 7 和 8 混用）。迁移时**一律向 gpui 刻度取整，禁止用 `px(n)` 复刻奇值**：

**间距**：

| pi-web | → 本项目 |
|---|---|
| 3 | `_1`(4px) |
| 5 | `_1p5`(6px) |
| 7 | `_2`(8px) |
| 9 | `_2p5`(10px) |
| 11 | `_3`(12px) |
| 18 | `_4`(16px) |

**圆角**（四档制的用途见 § 3.3，该表是唯一定义，此处只给收敛映射）：

| pi-web 圆角 | → 本项目 |
|---|---|
| 3 | `rounded_sm`(4px) |
| **5**（最高频，58 处） | **`rounded_md`(6px)** |
| 7 / 9 | `rounded_lg`(8px) |
| 10 / 11 / 12 / 14 | `rounded_xl`(12px) |

`_2p5`（10px）**必须保留**——pi-web 里 padding 用了 84 次、gap 用了 20 次，是真实高频档，不许并入 8 或 12。
它是 gpui 原生刻度（`rems(0.625)`，`ZED_crates/gpui_macros/src/styles.rs:954`），已登记进 § 3.1 刻度表，用它不需要 `px(n)`。

注意 gpui 链式 `rounded_sm` 是 **4px**，而 gpui-component 的语义 radius token `Theme::radius_tokens().sm` 是 **3px**（`radius/2`）——**两套不要混用**，本项目统一用 gpui 链式方法。

来源：`PIWEB_components/**` 内联样式实测频次；`GPC_crates/ui/src/theme/mod.rs:297-303`（radius_tokens）

### 3.2 各场景节奏（消息流 / 列表行 / 卡片 / composer）

**本表是纯索引，不含任何数值**（v2.3 起）。数值一律去定义点查：

| 场景 | 定义点 |
|---|---|
| 消息列（容器） | S-13（§ 5.1） |
| 助手消息条目 | § 5.1「助手消息」 |
| 用户消息条目 | S-14（§ 5.1） |
| 缩进消息（子代理/处理详情、工具输出、thinking 内容） | § 5.3 |
| 列表行（侧栏会话） | § 4.3；颜色见 S-7 |
| 工具卡片 | § 5.2；卡片通用结构见 § 4.2 |
| thinking 卡片 | § 5.4 |
| 代码块 | § 4.2（结构）+ § 3.3（圆角） |
| composer | § 5.6 |
| minimap / 目录节点 | § 5.5 |

### 3.3 圆角刻度

**四档制**（本表是全文唯一的圆角定义，其余章节只许引用）：

| 本项目方法 | 像素 | 用途 |
|---|---|---|
| `rounded_sm` | 4px | 按钮、minimap 节点、小 chip、徽章 |
| `rounded_md` | 6px | 输入框、工具卡、代码块、列表行、**非用户消息的一切卡片** |
| `rounded_lg` | 8px | 面板 |
| `rounded_xl` | 12px | **用户消息气泡**、**composer 输入壳**、**对话框** |

另有两个专用值：`rounded_xs`(2px) 仅用于极小元素、`rounded_full`(9999px) 仅用于状态点/头像/圆钮。

来源：`ZED_crates/gpui_macros/src/styles.rs:1228-1275`（实测：xs=rems(0.125)=2、sm=rems(0.25)=4、md=rems(0.375)=6、lg=rems(0.5)=8、xl=rems(0.75)=12）

> **勘误**：v2.0 及更早在本表写「gpui-component `radius_lg` 默认 12px」，**错误**——`GPC_crates/ui/src/theme/mod.rs:460` 实为 `radius_lg: px(8.)`，12px 对应的是 `radius_tokens().xl`（`radius * 2`）。

**规范 S-11**：圆角只用上表刻度，禁止自定义中间值（包括 5px、7px、9px、14px）；相邻嵌套元素圆角保持「同心收缩」（如面板 `rounded_lg` 内嵌卡片用 `rounded_md`）。

### 3.4 字号 / 字重 / 行高

| 用途 | Zed 语义字号 | 本项目 div 方法 | 像素 | 行高参考 | 字重 |
|---|---|---|---|---|---|
| 次文本 / 元信息 / 最小辅助文本 | TextSize::Small | `text_xs` | 12px（**全应用字号下限**，见 S-21） | 16px | normal |
| 常规 UI 文本 | TextSize::Default（14px） | `text_sm` | 14px | 20px | normal |
| 消息正文 | 同常规 | `text_sm`（**唯一允许值**，见 S-12） | 14px | `relative(1.7)` ≈ 24px | normal |
| 强调标题 | Headline::Small（16px） | `text_base` + `font_semibold` | 16px | 24px | semibold |
| 面板标题 | Headline::Medium（18px） | `text_lg` + `font_semibold` | 18px | 28px | semibold |
| 大标题 | Headline::Large/XLarge | `text_xl`+（20px） | 20px | 28px | semibold |

**规范 S-12（正文字号必须显式声明）**：`Root` 会执行 `window.set_rem_size(cx.theme().font_size)`，而 gpui-component 的 `Theme::font_size` 默认是 **16px**——任何不显式调 `text_*` 的元素都会落到 16px，而不是本表的 14px。因此**消息正文容器必须显式 `.text_sm()`**，不允许靠继承；markdown 标题由 `TextView` 按正文字号等比缩放，无需另设。

**规范 S-18（行高必须显式给，且比组件库默认宽松）**：阅读密度由字号与行高**共同**决定，只改字号不改行高，观感仍然偏挤。

| 场景 | 行高 | 说明 |
|---|---|---|
| **消息正文 / markdown** | `.line_height(relative(1.7))` | pi-web `globals.css:348` 实测值；14px × 1.7 ≈ 24px |
| markdown 标题 | `relative(1.35)` | `PIWEB_globals.css:359` |
| 用户气泡正文 | **同正文 `relative(1.7)`** | 与正文共用常量，见下 |
| 紧凑列表行 / 状态栏 | 不设（用组件默认） | 只有正文与气泡需要显式行高 |

gpui-component 语义 typography token 的行高是**绝对 px**（sm = 14/20 → 倍数 **1.43**），明显比 pi-web 正文的 1.7 紧。**正文不得吃 token 默认行高**，必须显式写 `relative(1.7)`。

gpui 的 `line_height(relative(x))` 语义等同 CSS `line-height: x`（按字号倍乘），但结果会 `.round()` 取整（1.7 × 14 = 23.8 → 24px）。

**正文与用户气泡共用同一个常量 `BODY_LINE_HEIGHT`（1.7）**，不许各写各的（S-25 有对应断言）。

来源：`PIWEB_app/globals.css:348, 359`、`PIWEB_components/MessageView.tsx:382`、`GPC_crates/base/src/theme_tokens.rs:112-117`、`ZED_crates/gpui/src/style.rs:554-556`

**规范 S-21（12px 是字号下限）**：pi-web 大量使用 `11px`（126 处）、`10px`（55 处）、甚至 `9px` 的极小字——时间戳、耗时、角标、行号都在这一档。**本项目一律抬到 `text_xs`(12px)**，不为了像素级还原基线而破坏字号刻度、牺牲桌面端可读性。需要弱化时改用**颜色**（dim 级，S-16）而不是继续缩字号。

- 禁止用调小 `Theme::font_size` / rem 的方式整体缩字：§ 3.1 的间距刻度、§ 3.3 的圆角刻度全部是 rem 派生，改 rem 会把 `gap_1`=4px 变成 3.5px、`rounded_md`=6px 变成 5.25px，整套刻度失真。要小字就按本表逐处声明 `text_sm` / `text_xs`。
- 字重：正文 normal；标题 semibold；**禁止**在 14px 以下使用 bold（Zed 只用 `font_semibold` 做标题，见 `ZED_crates/ui/src/styles/typography.rs` Headline 实现）。
- 等宽字体：`font_family(cx.theme().mono_font_family.clone())` 用于代码、工具输入输出 JSON、diff。**等宽字号一律 `text_xs`(12px)**——上游的 13px 不在 gpui 刻度上（只有 12 与 14），落地只能 `px(13.)`，违反红线 4。
- **工具名 / 卡片标题一律 `text_sm`(14px)**。上游用 13px，本项目就近取整到 14px 并全应用统一。

> 以上两条是**定值，不是二选一**。v2.0 及更早写作「13px 为佳」「用 `text_sm` 或 `text_xs` 就近取整」——要求统一却不指定统一到哪个值，等于没有约束力。

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

**尺寸（本表是全文唯一的按钮尺寸定义，其余章节只许引用不许复述）**：

| `Size` | 图标钮实际边长 | 带文字钮实际高 | 用在哪 |
|---|---|---|---|
| `XSmall` | `size_5()` = **20px** | `h_5()` = **20px** | 仅限**卡片内**的折叠/次要操作，且必须有 `.tooltip()` |
| `Small` | `size_6()` = **24px** | `h_6()` = **24px** | **行内操作、工具栏、composer 工具行的默认档** |
| `Medium` / `Large` | `size_8()` = **32px** | `h_8()` = **32px** | 主按钮（composer 发送、对话框确认） |

来源：`GPC_crates/ui/src/button/button.rs:529-552`（实测，非推算）。

**规范 S-23（可点区域下限 24px）**：纯图标按钮**最小用 `Small`(24px)**。`XSmall`(20px) 只允许出现在已经有大命中区的父行内部（如工具卡 header 整行可点，卡内 chevron 用 20px 无妨），**不允许作为独立可点目标**。

> 注：v2.0 及更早版本此处曾写「工具栏 `small`（h=28）」「≥22px」，均为**臆测值**——gpui-component 没有 28px 或 22px 档。凡按旧值实现的代码需复核。

- 图标按钮必须带 `.tooltip("…")`；tooltip 文案动词开头（「刷新」「隐藏目录」）。**无例外**（见红线 8）。
- 禁用态 `.disabled(busy)`；运行中禁止重复触发（本项目 `busy_actions` 模式）。
- 按钮内图标与文字间距 `gap_1`（4px）。

来源：`GPC_crates/ui/src/button/button.rs:145`（ButtonVariant）、`:48-73`（ButtonVariants）、`:529-552`（尺寸）

### 4.2 卡片（弱边框卡）

- 结构：`v_flex().p_2().rounded_md().border_1().border_color(cx.theme().border)`；
- 卡内第一段是 header：`h_flex().gap_1p5()`，header 与内容之间用 `gap_2` 或 `border_t_1`（内容多块时）；
- **禁止**：卡片内再套完整卡片（工具输出、diff 只能以左竖线/文本块下沉）。

**代码块**（本项目唯一定义处，v2.3 从 § 3.2 迁入）：内容 `p_2`、header `px_2 py_1`、`rounded_md`、
header 与内容之间 `border_b_1`；底色 `cx.theme().muted.opacity(0.42)`（见 § 2.3）；
正文走 `mono_font_family` + `text_xs`（见 S-21）。header 自绘的挂点是 `markdown_block_renderer`（见 § 4.8）。

### 4.3 列表行

- 结构：`div().p_2().rounded_md().cursor_pointer()`；
- 行与行之间 `gap_1`；
- hover：`.hover(|row| row.bg(cx.theme().muted))`；
- 选中：`.bg(cx.theme().accent.opacity(0.16))`，不加边框；
- 行内信息最多 3 片段：标题（`text_sm` 截断）+ 指标行（`text_xs` + `muted_foreground`）+ 状态图标色；
- 缩进（树形会话）：`ml(px(depth * 12.))`，与 minimap 缩进刻度（7px/级）不同用途，列表用 12px（红线 4 白名单）。

**S-19 豁免（v2.3）：会话列表不强制用 `List` / `ListItem`。**
§ 4.8 表把会话列表标为「复用 `List<ListDelegate>` + `ListItem`」，但本节的手写结构是**允许的**，理由有二：

1. `ListItem` 自带的选中色是 `list_active`，与 S-7 定死的 `accent.opacity(0.16)` **不是同一个值**——用组件就违反 S-7，手写才守得住；
2. 会话行需要「双行元信息 + 左竖条 + hover 显隐（红线 17 豁免的结构断言形态）」，与 `ListItem` 的形制不符。

§ 4.8 该行相应从「必须」降为「可选参考」。**其余场景的 S-19 强制不受影响。**

来源：`crates/app/src/session_sidebar.rs`（现状）、`ZED_crates/ui/src/components/list/`（行 hover 语义）

### 4.4 折叠（Disclosure / 手风琴）

- 折叠头：`h_flex().gap_1p5().cursor_pointer()`，图标 `ChevronRight/ChevronDown`（`IconName`），标题 `text_sm`；
- 折叠头 hover 反馈：`hover(|row| row.bg(cx.theme().muted))`；
- **chevron 常驻**（不 hover 显隐）——它是「这里可折叠」的唯一指示器，隐藏等于让可点区域不可发现；不再另给「展开/收起」文字，图标方向已经表达了状态；
- 内容区：展开时保持左对齐，与折叠头同宽；长内容走左竖线模式（见 5.3）。

### 4.5 状态点 / 状态竖条

- 状态点：`div().size_2().rounded_full().bg(cx.theme().<status>)`（8px 圆点）；
- 竖条（工具输出从属关系）：**结构与数值只在 § 5.3 定义**（`ml_1p5` + `pl_3p5` + `border_l_1`）；
- 状态点旁文字用 `text_xs + muted_foreground`，**状态色只上点/图标，不上文字**（或文字用 `text_xs` 同色，与点并列时不重复）。

### 4.6 Tooltip / Popover / 右键菜单

- Tooltip：一律组件内建 `.tooltip("…")`；触发延迟与样式走组件库默认，不自定义；
- Popover：浮层背景 `cx.theme().popover`、`rounded_md`、`border_1 + border`、阴影走组件库；浮层内列表项 hover 用 `secondary_hover`；
- 低频操作（删除/导出/更多）收右键菜单或「更多」popover，行内只留高频操作；
- 对话框：`window.open_dialog`，背景 `popover`，**遮罩用 `cx.theme().overlay`**（禁止硬编码 `rgba(0,0,0,.4)`），主按钮 `primary`、取消 `secondary`、危险 `danger`（`ok_variant(ButtonVariant::Danger)`，见 `session_sidebar.rs` 删除确认）；
- **层叠顺序由 `Root` 统一管理**（dialog / notification / popover 三层），业务代码不得干预、不得自排 z-index（红线 18）；
- **浮层定位一律交给 `Popover` 的 `anchor`**，它自带翻转避让与外点关闭，禁止手算坐标（红线 16）。

### 4.7 Divider

- 水平分隔：`Separator::horizontal()`（gpui-component `ui/src/separator.rs`）或 `div().h_px().w_full().bg(cx.theme().border.opacity(0.6))`；
- 垂直分隔：`Separator::vertical()`，用于工具栏分组、属性键值、状态栏 widget 之间；
- 分隔线不用于「套层级」，只用于同层分组；卡片与卡片之间用留白（`gap_3`+）而非分隔线。

来源：`GPC_crates/ui/src/separator.rs:28,68`、`ZED_crates/agent_ui/src/conversation_view/thread_view.rs:6471`

### 4.8 组件映射总表（UI 需求 → gpui-component）

**规范 S-19**：下表列出的场景**必须复用 gpui-component 现成组件**，不许手搓。

**已登记的两处豁免**（此外不许再自行认定豁免）：**会话列表**（理由见 § 4.3）、**折叠**（理由见本节末「结论」）。

表中所有**组件名与路径**已在 `GPC_CHECKOUT` 源码中核实存在；**括号内的像素值一律以源码为准，引用前请复核**——v2.0 曾在此表写错按钮尺寸（把 20/24/32 写成 26/28/30）。

| 场景 | 组件 / API | 关键说明 |
|---|---|---|
| 窗口标题栏 | `TitleBar::new()`（`ui/src/title_bar.rs:42`） | `TITLE_BAR_HEIGHT = px(34.)`；自动走 `title_bar` / `title_bar_border` token |
| 左侧栏外壳 | `Sidebar::new(id).side(..).collapsible(..).header(..).footer(..)`（`ui/src/sidebar/mod.rs:238`） | 默认宽 `px(255.)`；折叠钮用 `SidebarToggleButton`（`:302`） |
| 侧栏分组标题 | `SidebarGroup::new(label)`（`ui/src/sidebar/group.rs:17`） | 替代手写大写小标题 |
| 会话列表 | `List<D: ListDelegate>` + `ListItem`（`ui/src/list/`） | **可选参考，非强制**（S-19 豁免，理由见 § 4.3）。`ListDelegate` 一次覆盖 加载中/空/错误/分组/搜索/`load_more` 六态且自带虚拟化，值得参考；但 `ListItem` 的 `list_active` 与 S-7 定死的 `accent.opacity(0.16)` 冲突，且装不下双行 + 左竖条。**`SidebarMenuItem` 一律不用**（见下表） |
| 列表行 hover/选中 | `ListItem::new(id).selected(..)` | 自带 `list_hover` / `list_active`。**用它就吃它的选中色，与 S-7 的 `accent.opacity(0.16)` 二选一**——会话列表按 § 4.3 走手写 + S-7 |
| 文件树 | `tree(&state, ..)` + `TreeItem` / `TreeState`（`ui/src/tree.rs:18`、`crates/base/src/tree.rs:97`） | **不自动缩进**——`TreeEntry::depth()` 拿层级后自己 `.pl(..)`；展开态由 `TreeState` 托管，不要自管集合 |
| 树/列表右键菜单 | `Tree::context_menu(..)`（`ui/src/tree.rs:55`）、`PopupMenu`（`ui/src/menu/popup_menu.rs:283`） | 低频操作收这里，行内只留 1~2 个 |
| 多行输入 | `Textarea::new(&state)`；state 用 `TextareaState::new(..).auto_grow(min,max).submit_on_enter(true)`（`crates/base/src/input/base/state.rs:4873`） | `auto_grow` 直接替掉手写的 `min(scrollHeight, N)` 高度同步 |
| 输入框外观自绘 | `Textarea::new(..).appearance(false).bordered(false)`（`ui/src/textarea.rs:54,59`） | 外面自己包容器画圆角/边框/阴影 |
| 单行输入 | `Input::new(&state)` / `TextInput` | 重命名等原地编辑 |
| 只读长文本 | `Editor::new(&state).readonly(true).appearance(false)`（`ui/src/input/editor.rs:33`） | system prompt 等 |
| 按钮 | `Button::new(id)` + `ButtonVariants`（`ui/src/button/button.rs:48-73`） | `.primary()` / `.secondary()` / `.ghost()` / `.danger()` / `.warning()` / `.info()`；**尺寸见 § 4.1 表，此处不复述** |
| 按钮组 | `ButtonGroup::new(id)`（`ui/src/button/button_group.rs:44`） | 三等分入口行等 |
| 二选一 / 多选切换 | `Toggle` / `ToggleGroup::new(id).segmented()`（`ui/src/button/toggle.rs:50,234`） | **注意 `ToggleGroup` 是多选语义**：回调回传的是整个勾选向量（点第 2 段会得到 `[true, true]`），要做单选必须与当前状态差分定位被点段 |
| 带下拉的按钮 | `DropdownButton::new(id).button(..).dropdown_menu(..)`（`ui/src/button/dropdown_button.rs:37`） | 模型 / thinking / tools 选择器 |
| 浮层 | `Popover::new(id).anchor(..).trigger(..).content(..)`（`ui/src/popover.rs:43`） | **自带翻转避让与外点关闭**（`overlay_closable`），禁止手算坐标 |
| 悬浮信息卡 | `HoverCard`（`ui/src/hover_card.rs`） | 比塞进单行 tooltip 更合适的富内容 |
| Tooltip | `Button::tooltip(..)`（`button.rs:312`）、`Tooltip::new(..)` / `::element(..)` / `::key_binding(..)`（`ui/src/tooltip.rs:43,53,75`） | 次要信息一律进这里（S-8） |
| 快捷键显示 | `Kbd::new(..)` / `Kbd::binding_for_action(..)`（`ui/src/kbd.rs:30,52`） | — |
| 对话框 | `window.open_dialog(..)` / `window.open_alert_dialog(..)`（`ui/src/window_ext.rs:30,51`） | 按钮分级走 `DialogButtonProps::ok_variant(..)`；遮罩色用 `cx.theme().overlay`，禁止硬编码 |
| 通知 | `window.push_notification(Notification::success(..), cx)`（`ui/src/window_ext.rs:65`、`ui/src/notification.rs`） | 四档 Info/Success/Warning/Error。**pi-web 没有应用内通知（走浏览器 API），这是原生端应补上的能力** |
| 侧滑面板 | `window.open_sheet_at(Placement::Right, ..)`（`window_ext.rs:19`） | — |
| 底部状态栏 | `StatusBar::new().left(..).right(..)`（`ui/src/status_bar.rs:40`） | 已内建上边框 + `status_bar` 底 + `text_xs` + `muted_foreground`，与 pi-web 同构 |
| 标签页 | `TabBar::new(id)` + `Tab`（`ui/src/tab/`），或 `dock::TabPanel` | 后者带拖拽重排与分屏 |
| 可拖拽分栏 | `h_resizable(id)` + `resizable_panel()`（`crates/base/src/resizable/mod.rs:17`） | 一次替掉侧栏/右栏两套 CSS 变量与手写 resizer |
| 进度 | `Progress::new(id).value(..)`（`ui/src/progress/progress.rs:24`） | 上传进度、context 用量 |
| 加载中 | `Spinner::new()`（`ui/src/spinner.rs:20`）、`Skeleton`（`ui/src/skeleton.rs`） | — |
| 小圆点 / 角标 | `Badge::new().dot().color(..)`（`ui/src/badge.rs:43,56,82`） | 未读、目录含改动等 |
| 标签 / 药丸 | `Tag::new()` + `.outline()` / `.warning()` / `.info()`（`ui/src/tag.rs`） | 队列 kind、分支名 |
| 键值表 | `DescriptionList`（`ui/src/description_list.rs`） | 会话信息面板 |
| 表格 | `Table`（`ui/src/table/`） | **不要用它渲染 diff**——自带虚拟滚动与列宽交互，属嵌套滚动容器（S-20） |
| 复制 | `Clipboard`（`ui/src/clipboard.rs`） | 自带「复制 → 已复制」状态 |
| Markdown 正文 | `TextView::markdown(id, src)`（`ui/src/text/text_view.rs:120`） | 自定义块渲染器走 `markdown_block_renderer(..)`（`:256`）——这是 mermaid 直出源码与代码块自绘 header 的正确挂点 |
| 语法高亮 | `highlighter` 模块（`ui/src/highlighter/`） | — |
| 滚动条 | `Scrollbar` + `ScrollbarHandle`（`gpui_base` re-export） | 配色走 `scrollbar` / `scrollbar_thumb` / `scrollbar_thumb_hover` |
| 分隔线 | `Separator::horizontal()` / `::vertical()`（`ui/src/separator.rs:28`） | — |
| hover 显隐 | `.group("name")` + `.invisible().group_hover("name", \|s\| s.visible())` | **禁止用 Rust state 复刻 React 的 `hovered` 布尔**（用例：`ui/src/notification.rs:333,369`） |

**明确不用的组件**（已核实其形制与本规范冲突）：

| 组件 | 不用的理由 |
|---|---|
| `Collapsible`（`ui/src/collapsible.rs`） | 只有 `new/open/content` 三个方法，**无 header/trigger**，本质等于一个 `when` 包装，收益为零 |
| `Accordion` / `AccordionItem`（`ui/src/accordion.rs:25`） | 功能够，但自带一整套边框与标题样式，与「弱边框卡 + 自绘 header」的规范正面冲突 |
| `SidebarMenuItem`（`ui/src/sidebar/menu.rs:267`） | 单行 `text_sm` 的 label+icon+suffix，装不下会话行的双行元信息 + 左竖条 |
| 内置补全菜单 `CompletionProvider` | 只对 `EditorMode` 开放（`crates/base/src/input/editor/lsp/completions.rs:121`），`TextareaState` 拿不到。composer 的 `/` 与 `@` 面板须用 `Popover` + `List` 自建 |

**结论：折叠一律自绘**——`h_flex()` header + `Icon::new(ChevronRight/ChevronDown)` + `.when(expanded, ..)`。工具卡、thinking、处理详情组三处折叠全部走这个模式，不引入折叠组件。

---

## 5. AI 面板设计模式（消息流 / 工具卡 / thinking / minimap）

### 5.1 消息列与消息条目

#### 消息列（容器）

**规范 S-13**：消息流必须收在一条**居中、有最大宽度**的列里，不允许铺满窗口宽度。

```
div().w_full().px_4()                          // 列外 16px 左右留白
    .flex().justify_center()
    .child(div().w_full().min_w_0().max_w(px(820.)))   // 列本体
```

- `max_w` = **820px**，超宽窗口只增加左右留白，不增加行长；
- 列外 `px_4`（16px）保证窄窗口下文字不贴边；
- 该容器套在**每一个列表项**外层（消息、处理详情组都算），因为消息流是虚拟化 `list`，没有统一的内容父节点；
- 目的：控制行长在可读区间。通栏长行是「读起来累」的首因，与字号无关。

来源：`PIWEB_components/ChatWindow.tsx:703`（`maxWidth: 820, margin: "0 auto"`）、`:702`（`padding: 0 CHAT_COLUMN_PADDING(=16)px`）

#### 用户消息（右对齐气泡）

**规范 S-14**：用户消息是**右对齐的弱色气泡**，不通栏、不与助手消息共用排版。这是全应用**唯一**允许给消息本体上底色的地方。

```
v_flex().items_end().mb_4()                    // 整条右对齐
    .child(
        v_flex().max_w(relative(0.85))         // 气泡最宽 85%
            .px_3().py_2()                     // 12 / 8px
            .rounded_xl()                      // 12px
            .bg(cx.theme().blue.opacity(0.10))              // v2.2：走 base.blue
            .border_1().border_color(cx.theme().blue.opacity(0.2))
            .text_sm()                         // 14px，见 S-12
    )
```

> 气泡身份色**必须是 `blue` 而不是 `accent`**（`accent` 在 gpui-component 是中性 hover 色，10% 铺在画布上不可见，
> v2.2 T3 复验判红）。`blue` = base.blue，浅 blue-600 / 深 blue-400，与 pi-web `rgba(59,130,246,…)` 同族；
> 选中态边框用同源实色 `blue`。S-25 的饱和度断言就是为了防止这里再退化成中性灰。
> **其余 `accent` 消费点维持中性**（列表 / minimap 选中 0.16、焦点边框），未决部分见 § 2.1 待决项。

- **不显示 `User` 角色标签**——右对齐 + 气泡底色已经完成了身份区分，再加标签就是重复编码，且违反 S-8；
- 气泡内正文 `text_sm`、行高走 `BODY_LINE_HEIGHT`（见 S-18）；
- 图片附件在正文上方，`gap_1p5`、`rounded_md`、最大 240×240；
- 行内操作（复制/编辑/分叉）与时间戳放气泡下方右侧一行，hover 才显形（S-9）。

来源：`PIWEB_components/MessageView.tsx:365`（`alignItems: flex-end`、`marginBottom: 16`）、`:371`（`maxWidth: "85%"`）、`:375-385`（底色/边框/圆角 12/内距 8-12/字号 14；**行高 1.6 是 pi-web 值，本项目统一用 1.7，见 S-18**）、`PIWEB_app/globals.css:32,49`（`--user-bg`）

#### 助手消息

```
v_flex().w_full().px_5().py_1p5().gap_2()
    .text_sm().line_height(relative(1.7))   // 无背景无边框
    .child(模型名行)                          // 见 S-20
    .child(v_flex().gap_2().children(blocks))
    .child(底部行：用量 · 复制 · 时间戳)
```
- 助手消息是纯文本流，不加卡片、**不加气泡、不右对齐**——左满、右让，靠这个不对称让人一眼分清谁在说话；
- 正文显式 `text_sm`（14px）+ `line_height(relative(1.7))`（**两者都不得省略，见 S-12 / S-18**）；
- 块与块之间 `gap_2`；
- 角色标签保留给需要提示的角色：Compaction/BranchSummary = `warning`；**Unknown 用 `warning` 而非 `danger`**——「渲染不了」不等于「出错了」，不该跟真错误抢同一个视觉通道；
- 首条消息顶部留 `pt_2`，末条底部 `pb_4`。

**规范 S-20（助手消息默认展示模型名）· 生效轮次：R10 起**：模型名是助手消息的默认组成部分，不是可选装饰。

> **本条在 R10 之前不作为判红依据**（数据通路尚不存在，见下方实现依赖）。凡未生效条款全文统一标注「生效轮次」。

| 维度 | 规定 | pi-web 基线 |
|---|---|---|
| 位置 | 消息**上方**独占一行，左对齐，在正文之前 | `MessageView.tsx:726-739` |
| 字号 | `text_xs`（12px） | 基线 11px，抬到本项目 12px 下限（S-21） |
| 颜色 | **dim 级** `muted_foreground.opacity(0.7)`（见 S-16） | `--text-dim` |
| 下间距 | `mb_1`（4px） | `margin-bottom: 4` |
| 显示条件 | **模型信息缺失时整行不渲染**，不留空位、不显示占位符 | 仅 `provider` 为真才渲染 |
| 取值优先级 | 显示名 → 原始 id | `modelNames["{provider}:{model}"]` → `modelNames[model]` → `message.model` |
| 与右侧指标 | 流式态可在同行右侧追加吞吐指标，`gap_1p5` | 同上 |

**一行不超过 3 片段（S-8）在这里是硬约束**：流式态下该行已经是「模型名 + token 数 + 吞吐徽章」三段，**再要加思考等级、上下文占比等信息必须进 tooltip**。

**实现依赖（跨 crate 分层，必须由 R10 任务卡授权）**

本条是 UI 规范里**唯一一条要求改动非 GPUI crate 的条款**，因此把接口写死在这里，避免 R10 再考古：

| 现状 | 位置 |
|---|---|
| `pi-data` **已**解析 `model_change`（`provider` + `model_id`） | `crates/pi-data/src/session.rs:66-70, 357-362` |
| `pi-render` 把它与其它元数据一并丢弃（`_ => {}`） | `crates/pi-render/src/lib.rs:447` |
| `Message` 结构体**无**模型字段（只有 `id/role/timestamp/label/blocks`） | `crates/pi-render/src/lib.rs:78-84` |

需要的变更（R10 范围）：`pi-render` 在折叠条目时跟踪「当前模型」，并给 `Message` 增加 `model: Option<ModelRef>`（`ModelRef { provider: String, id: String }`）。`model_change` 条目本身仍不占正文，只更新游标。

**这属于改渲染中间模型**，被 R9 任务卡「禁止」条明令排除，且 `pi-render` 是自动化验收的基础 crate（不依赖 GPUI、可全量单测）。**R9 内任何人不得据此条改动 `pi-render`**；R10 承接时须在其任务卡里显式授权该变更。

**处理详情/子代理输出**（参考 Zed 缩进消息 `thread_view.rs:6506`）：
- 走 § 5.3 的左竖线 + 缩进（`ml_1p5` + `pl_3p5` + `border_l_1`），**不铺底色**——底色会让它变成第四级背景，违反 S-3；
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
- header 底色：**不铺底**（与 § 2.3 一致）；
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
- 输出内小块间用 `gap_2`；Input JSON、输出文本用等宽字体 + `text_xs`(12px)（见 S-21）；
- 多段输出（Text/Ansi/Image/Diff）同竖线区内顺序排列，`gap_2`。

### 5.4 Thinking 折叠

参考 `thread_view.rs:7434`（render_thinking_block）：

- header：`h_flex().gap_1p5().cursor_pointer()`，图标 `IconName::ToolThink`（或灯泡）小尺寸 + `muted_foreground`，标题「思考」`text_sm + font_semibold`；
- chevron 在右侧，**常驻**（见 § 4.4；hover 显隐会重新引入 § 6.1 #4 点名要修的可发现性缺陷）；
- 展开内容：左竖线模式（同 5.3），`max_h(px(256.))` + 底部渐变遮罩；**遮罩态下不给内部滚动，改配「展开全部」按钮**——点击后去掉 `max_h`，让表项真实变高（理由见 S-22）；
- **渐变遮罩的实现**（这是全文唯一的渐变，写死避免各写各的）：容器 `relative()`，底部叠一个绝对定位覆盖层，`h(px(32.))` + `bottom_0 left_0 right_0`，背景 `linear_gradient(0., linear_color_stop(cx.theme().background, 0.), linear_color_stop(cx.theme().background.opacity(0.), 1.))`。**不要试图用 `mask-image`**——GPUI 无此能力（§ 5.7）；
- thinking **不套卡片边框**（它是消息流内的次级内容），只用左竖线区分层级；
- 基线的 thinking 折叠头**没有任何折叠指示器**（`PIWEB_MessageView.tsx:910-930`），本项目**必须补 chevron**——无指示器的可点区域是可发现性缺陷，不照搬。

### 5.5 Minimap / 目录

- 面板：`w(px(176.))`、`border_l_1 + border`、`bg(sidebar)`（`chat.rs` 现状已符合）；
- 节点：`px_2 py_1 rounded_sm text_xs truncate`，缩进 `ml((level-1) * 7px)`，`gap_1`；
- 缩进：`ml((level - 1) * 7px)`（**唯一定义处**；`px(7.)` 已在红线 4 白名单）；
- 选中：`bg(accent.opacity(0.16))` **+ 左侧 2px `accent` 竖条，绝对定位不占布局**（占布局会让选中切换时整列横向抖动）；hover：`bg(muted)`；点击滚动到消息并保持焦点在消息列表；
- 目录面板顶部：关闭按钮 `ghost + xsmall + icon`，`justify_end + p_1`（现状已符合）。

### 5.6 Composer

参考 `PIWEB_components/ChatInput.tsx` 与 `thread_view.rs:4333`：

- **列宽与消息列一致**：`max_w(px(820.))` 居中（`ChatInput.tsx:1382`）——composer 与消息流必须同宽同轴，否则视觉上是两个系统；
- 容器：`border_t_1 + border`、`bg(background)`、内容 `px_2 py_2`；
- **输入壳**：`rounded_xl`(12px)、`border_1`、`shadow_sm`（阴影豁免见 S-2）；越靠近输入焦点圆角越大是基线的一致规律；
- **输入区高度走 `TextareaState::auto_grow(1, 8)`**（`crates/base/src/input/base/state.rs:4873`）——1 行起、约 8 行封顶，不要手写 `scrollHeight` 同步；
- 工具行控件统一 `ghost` + `Size::Small`（尺寸见 § 4.1 表），图标钮正方；
- 左 = 附件 / 模型选择器；右 = 模式切换（`ToggleGroup::segmented()`）+ 发送主按钮（`primary`，唯一常驻主操作）；
- **停止按钮只在运行态出现**，用 `.danger()`；
- placeholder 按状态切换文案（空闲 / 可 steer / 运行中不可 steer），色 `muted_foreground`；
- 附件 chip：`rounded_md + border_1 + border`，缩略图 56×56；
- **`/` 与 `@` 面板用 `Popover` + `List` 自建**——内置补全菜单只对 `EditorMode` 开放，`TextareaState` 用不了（见 4.8）。

### 5.7 虚拟化 list 的硬约束

**规范 S-22（禁止在消息流表项内嵌套滚动容器）**：这是从 pi-web 迁移到 GPUI 时**最集中的一类破坏性差异**，单列一条。

消息流是虚拟化 `list(ListState, ..)` + `ListSizingBehavior::Infer`。表项内部一旦出现「固定 `max_h` + 内部滚动」，会同时造成两个后果：

1. list 把表项高度量成那个固定值而非真实内容高度 → 滚动条比例失真、`scroll_to` 定位偏移；
2. 内层容器吞掉滚轮事件 → tail-follow 与「向上滚脱离」判定失效。

pi-web 基线里这样的容器共 **9 处**（用户气泡 300、超大消息 420、工具结果 400、diff 双栏 560、diff 单栏 520、custom 详情 360、compaction 文件列表 180、minimap 预览层、以及本规范 v1.0 曾从 Zed 抄来的 thinking 256）。

**这 9 处的「内部滚动」一处都不照搬；`max_h` 作为纯视觉截断可以保留**（配渐变遮罩 + 展开按钮，见 § 5.4）。禁的是滚动，不是高度上限。

统一替代模式——**截断 + 展开**：

```
默认：只渲染前 N 行（或 max_h + 底部渐变遮罩），配「展开全部」按钮
展开：去掉高度限制，表项真实变高，list 重测一次
```

这样既保留「不被超长内容淹没」的意图，又不引入嵌套滚动。

**连带禁令**：不要用 `Table` 渲染 diff（它自带虚拟滚动与列宽交互，本质也是嵌套滚动容器）。

**另一条同源约束**：折叠/展开会让表项高度剧变（24px → 数千 px），切换后必须主动触发重测并保持滚动锚点，否则视口会跳飞。这是 R9 之后最容易回归的点。

**GPUI 无等价物、不要试图模拟的 CSS 能力**：`position: sticky`（多文件 diff 的文件头改为「每文件一个折叠块」）、`backdrop-filter: blur`（用不透明 `popover` 底替代）、`filter: brightness`（改用明确的 hover token）、`mask-image`（`Icon` 本就按 text color 着色 SVG）。

### 5.8 条款 → 断言（可验证性）

**规范 S-25**：影响面最大的视觉条款**必须有机械可验证的落地形式**，不许只靠目视。

现有测试基建（`debug_selector` + `debug_bounds`）只返回**位置与尺寸**，断不了颜色、行高、`max_w`。因此凡涉及颜色/排版的条款，必须把值抽成**可在 `cx.update` 中断言的纯函数或常量**，再由测试比对。下表是硬性要求，评审照单验：

| 条款 | 落地形式 | 断言方式 |
|---|---|---|
| **S-13** 消息列 820 居中 | 列容器加 `debug_selector("message-column")` | 窗口宽 > 852 时断言 `bounds.size.width == px(820.)`；窄窗口断言 `width == 窗口宽 - 32` |
| **S-14** 用户气泡 | 抽 `pub(crate) fn user_bubble_style(cx) -> UserBubbleStyle { bg, border, selected_border, radius, max_w_ratio }`（v2.2：基色为 `cx.theme().blue`） | `cx.update` 直接断言各字段；深浅两种模式下断言边框饱和度 ≥ 0.5（防再次回归成中性灰，v2.2）；另给气泡加 `debug_selector("user-bubble")`，断言其右缘贴列右缘、宽度 ≤ 列宽 × 0.85 |
| **S-18** 行高 | 抽 `pub const BODY_LINE_HEIGHT: f32 = 1.7;` | 断言常量值，并断言消息正文与用户气泡两处都引用它（不许各写各的） |
| **S-16** 三/四级文本 | `dim_foreground(cx)` / `disabled_foreground(cx)` 两个函数 | 断言返回值 = `muted_foreground.opacity(0.7 / 0.5)`；grep 断言组件内无裸 `muted_foreground.opacity(` |
| **S-15** 深色面板层级 | 主题投影集中在 `crates/ui/src/theme.rs` | 深色下断言 `theme.sidebar != theme.background` |
| **S-23** 可点区域 | — | 断言行内图标钮 `debug_bounds` 高度 ≥ 24px |
| **S-22** 无嵌套滚动 | — | 结构断言：消息表项内不出现第二个滚动容器的 `debug_selector` |

**只能目视的条款**（无法机械化，必须进 T3 目视清单，不许在 T2 里假装覆盖）：具体色值的观感、深浅两套的整体协调、渐变遮罩的过渡效果、字体渲染质量。

---

## 6. 禁止事项（红线）

**本章是纯指针**（v2.3 起）——每条只写禁什么 + 定义点，不复述数值与条件。
判红时以指向的条款为准；条款改了，红线自动跟着改。

| # | 禁止 | 定义点 |
|---|---|---|
| 1 | 硬编码颜色（唯一例外：ANSI 16 色，且必须在 `chat.rs::ansi_color` 内统一扩展） | § 2 全章 |
| 2 | 边框套边框（卡片内套完整描边卡片） | § 4.2、§ 5.3 |
| 3 | 状态色铺满（作卡片整体边框色或铺满背景） | S-4 / S-10；`accent` 与 `blue` 不受此条约束，见 S-24 |
| 4 | 自造字号 / 圆角 / 间距 | § 3.1、§ 3.3、§ 3.4；`px(n)` 白名单见下 |
| 5 | UI 字体混用 | § 3.5 |
| 6 | 14px 以下用 bold | S-21 |
| 7 | 一行超 3 片段 | S-8（含「片段」判定规则） |
| 8 | 无 tooltip 的纯图标按钮（**无例外**） | § 4.1 |
| 9 | 无状态反馈的 hover 空白 | S-7、§ 4.4 |
| 10 | 除用户消息外的任何角色上卡片底 / 气泡 / 右对齐（Assistant / Compaction / BranchSummary / Custom / Unknown 全适用） | S-14 是唯一例外，形态以 S-14 为准 |
| 11 | 消息流通栏 | S-13 |
| 12 | 消息正文靠继承取字号 / 行高 | S-12 / S-18 |
| 13 | 消息流表项内嵌套滚动容器 | S-22 |
| 14 | 低于 12px 的字号 | S-21 |
| 15 | 手搓已有组件 | S-19（含两处已登记豁免） |
| 16 | 手算浮层坐标 | § 4.6 |
| 17 | 每元素各配一个 `hovered: bool`（默认走 `.group()` + `.group_hover()`） | § 4.8 末行；**豁免见下** |
| 18 | 在业务代码里排 z-index | § 4.6 |

**红线 4 的 `px(n)` 白名单**（此外一律不许；新增必须改本表）：

| 值 | 用途 | 定义点 |
|---|---|---|
| `176` | minimap 面板宽 | § 5.5 |
| `7` | minimap 缩进 / 级 | § 5.5 |
| `12` | 会话树缩进 / 级 | § 4.3 |
| `820` | 消息列 `max_w` | S-13 |
| `256` | thinking `max_h` | § 5.4 |
| `32` | 渐变遮罩高 | § 5.4 |
| `240` | 消息内图片附件边长上限 | S-14 |
| `56` | composer 附件缩略图边长 | § 5.6 |

**红线 17 的豁免**：当验收要求以「元素不在渲染树中」做结构断言时（`debug_bounds` 返回 `None`），必须用 hover state + `.when()`——
`.group_hover()` 配 `.invisible()` 的元素**仍占布局、`debug_bounds` 仍返回 `Some`**，两者不可兼得。
此时 state 必须收敛到单个 `hovered_id: Option<Id>`，不得每行一个布尔。本项目 `session_sidebar.rs` 的行内操作显隐即属该豁免。

### 6.1 明确不照搬 pi-web 的部分

pi-web 是功能与阅读体验的对照基线，**不是逐像素复刻的对象**。下列做法经调研判定为基线自身的缺陷或 web 特有约束，**本项目明确不跟随**；后续轮次不得以「pi-web 就是这么做的」为由改回去。

| # | 基线做法 | 出处 | 不跟随的理由 |
|---|---|---|---|
| 1 | 工具卡用状态色铺满边框 + 底色（成功绿框绿底 / 失败红框红底） | `MessageView.tsx:972-973` | 违反 S-4 / 红线 3。一屏十几张卡片时每张都在视觉上尖叫。改统一弱边框 + header 状态点 |
| 2 | 工具输出靠「同卡片内 `border-top` 分区」表达从属 | `MessageView.tsx:1019,1291` | 背景在卡片底/输入/输出之间来回跳，层级读起来是**平的**。改左竖线 + 缩进（S-11 / 5.3） |
| 3 | 处理详情组展开后**零缩进** | `ChatWindow.tsx:245` | 展开后与普通消息混在一起，没有任何线索表明它们属于该组。必须补左竖线 |
| 4 | thinking 折叠头无 chevron | `MessageView.tsx:910-930` | 可发现性缺陷。必须补 |
| 5 | 工具卡**没有「运行中」视觉**（无结果的卡与成功卡外观完全一致） | `MessageView.tsx:953` | 基线缺口。本项目保留 pending 状态点 |
| 6 | 未知内容不上任何状态色 / 或反过来上 `danger` | `MessageView.tsx:1599` | 两个极端都不对。用 `warning` 级状态点——「渲染不了」不等于「出错了」 |
| 7 | 消息流内不解析 ANSI（转义序列当普通字符显示） | `lib/ansi.ts` 仅用于扩展面板 | 基线缺口。本项目 `pi-render` 已有 ANSI 中间模型，继续保留 |
| 8 | minimap 用 36px 极窄轨道 + hover 向左飞出 320px 预览层 | `ChatMinimap.tsx:22`、`ChatMinimap.module.css:1-19` | 本项目是 176px 常驻目录面板，信息已给足；再加 hover 飞出属重复编码。**两条路线不许混** |
| 9 | 大量硬编码色（`rgba(59,130,246,…)`、`#16a34a`、`#53b3cb`、`rgba(128,128,128,…)`） | 全仓 | 绕开主题系统，深浅色下不自适应。全部映射到 `cx.theme()` |
| 10 | 11px / 10px / 9px 极小字 | 全仓 | 桌面端可读性不足，见 S-21 |
| 11 | 移动端适配（`isMobile` 分支、`vh`/`dvh`、`safe-area-inset`、浮层抽屉、More 折叠工具条、三档媒体查询） | `ChatInput.tsx`、`AppShell.tsx`、`globals.css:1250+` | 桌面端一套布局 + 可拖宽度即可 |
| 12 | 应用内无通知系统（走浏览器 `Notification` API） | `AppShell.tsx:645-705` | 基线缺口。原生端用 `window.push_notification` 补上 |
| 13 | 全程用原生 `title=` 做 tooltip | 全仓 | 改用组件库 `Tooltip`，样式统一、可放富内容与快捷键 |
| 14 | 死变量 `--accent-hover` / `--assistant-bg`（零消费点） | `globals.css:13,15` | 不迁移，也不在本项目 theme 里预留对应 token |

**值得照搬的 4 条**（反过来也记明，避免被当成可选项砍掉）：

1. **820px 居中列 + 16px 外留白**（S-13）——控制行长，长文可读性的首因；
2. **live tail 不分组**（`ChatWindow.tsx:806-813`）——运行中的过程必须完全可见，折叠只施加于已完成的轮次。这是正确的产品判断，不是实现妥协；
3. **NoticeShelf 的「中性容器 + 7px 状态点」**（`ChatWindow.tsx:965-1003`）——全基线最符合「状态色只点不铺」的实现；
4. **minimap 大纲的丢弃规则**（`ChatMinimap.tsx:100-121`）——只留 h1–h3，无标题则只留首段。信息密度靠**丢弃**换来，不是靠塞。

---

## 7. 来源文件索引（复核用）

**各条款的出处写在该条款正下方的「来源：」行**——那才是复核某个具体数值时该看的地方。
本节只留两样在别处找不到的东西：**版本钉死表**，和**没有对应条款、但调研时用得上的参考位置**。

> v2.3 精简：原表 34 行里有 ~20 行是逐条「来源：」行的按主题重排，删。

### 7.1 路径前缀与版本钉死

| 前缀 | 版本（以 `Cargo.lock` 为准） | 本地路径 |
|---|---|---|
| `ZED_` | `cc053a4a6fa2fd0e8793201ed9099466af1be0b1` | `~/.cargo/git/checkouts/zed-a70e2ad075855582/cc053a4/` |
| `GPC_` | `000114aad412b1a1b26cb65cd0c8ae9467fd396a` | `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/000114a/` |
| `PIWEB_` | `v0.8.9` / `2a6e53710f6409e0cceb3de839a62f8cdf3ca3ca` | `vendor/upstream/pi-web-0.8.9/`（仓库内，`scripts/fetch-pi-web.ps1` 准备，`pins/*.manifest` 全量校验） |

**本文所有 `ZED_` / `GPC_` 行号以上表 rev 为准。** `~/.cargo/git/checkouts/` 是会被 cargo 回收的本地缓存，
路径本身不可依赖——复核时先用 `Cargo.lock` 里的 sha 确认 rev，再定位。
三者的 sha 由 `scripts/check-pins.*` 校验（`ZED_`/`GPC_` 校验 `Cargo.lock`，`PIWEB_` 校验目录内容）。

以上均为只读调研路径，本规范不复制任何 Zed / gpui-component / pi-web 代码，仅提炼设计语言与实测数值。

### 7.2 无对应条款的参考位置

本表列出的东西**本规范没有为其立条款**，但调研或后续扩展时会用到；列在这里以免被当成遗漏而重新考古。

| 主题 | 文件 | 说明 |
|---|---|---|
| 动画时长（50/150/300ms） | `ZED_crates/ui/src/styles/animation.rs` | 本规范**未规定动画**，全文无对应条款 |
| Zed UI 密度档位 | `ZED_crates/theme/src/ui_density.rs` | 本项目只用默认密度，未做密度切换 |
| Zed 按钮底层样式 | `ZED_crates/ui/src/components/button/button_like.rs` | § 4.1 的「对应 Zed」列出自此；本项目尺寸一律以 `GPC_` 实测为准 |
| Zed 工具卡的其余分支 | `ZED_crates/agent_ui/src/conversation_view/thread_view.rs:9961`、`:10179` | § 5.2 只提炼了主结构 |
| pi-web 助手消息全段（用量 `:803`、时间戳 `:841`） | `PIWEB_components/MessageView.tsx:718-842` | S-20 只提炼了模型名行 `:726-739` |
| pi-web 工具卡全段 | `PIWEB_components/MessageView.tsx:953-1315` | § 6.1 #1/#2/#5 只摘了其中三处 |
| pi-web thinking 全段 | `PIWEB_components/MessageView.tsx:904-945` | § 5.4 / § 6.1 #4 只摘了无 chevron 那处 `:910-930` |
| pi-web 处理详情分组 | `PIWEB_components/ChatWindow.tsx:195-245, 784-877` | § 6.1 #3 与「值得照搬」#2 各摘一处 |
| pi-web 用户气泡 `max-height: 300` | `PIWEB_components/MessageView.tsx:147` | S-22 列为 9 处嵌套滚动之一，本项目不照搬 |
| pi-web 全局基础字号 `14px` | `PIWEB_app/globals.css:109` | 本项目**不跟随**这种 rem 整体缩放，理由见 S-12 / S-21 |
| pi-web 消息列留白常量 | `PIWEB_components/ChatWindow.tsx:72`（`CHAT_COLUMN_PADDING = 16`） | S-13 引用的是其消费点 `:702` |
| pi-web diff（双栏 / 单栏兜底） | `PIWEB_components/MessageView.tsx:1065-1263` | diff 渲染细则尚未成条款，只在 S-22 里禁了嵌套滚动与 `Table` |
| pi-web composer 三个弹层 | `PIWEB_components/ChatInput.tsx:1534-1860` | § 5.6 只规定了 `/` 与 `@` 面板的实现路径 |
| pi-web 会话列表 / 文件树 / 顶栏 / 状态栏 | `PIWEB_components/SessionSidebar.tsx`、`FileExplorer.tsx`、`AppShell.tsx`、`ExtensionStatusBar.tsx` | 外围面板的功能对照基线 |
| pi-web minimap 大纲丢弃规则 | `PIWEB_components/ChatMinimap.tsx:100-121` | 结论已进 § 6.1「值得照搬」#4 |
| pi-web NoticeShelf | `PIWEB_components/ChatWindow.tsx:649-663, 957-1003` | 结论已进 § 6.1「值得照搬」#3 |
| gpui-component 把 rem 钉到 `Theme::font_size` | `GPC_crates/ui/src/root.rs:547` | S-12 的成因 |
| gpui-component 组件清单（§ 4.8 映射依据） | `GPC_crates/ui/src/`：`sidebar/`、`list/`、`tree.rs`、`button/`、`popover.rs`、`hover_card.rs`、`tooltip.rs`、`dialog/`、`notification.rs`、`status_bar.rs`、`tab/`、`progress/`、`spinner.rs`、`badge.rs`、`tag.rs`、`separator.rs`、`description_list.rs`、`clipboard.rs`、`kbd.rs`、`text/`、`highlighter/`；`GPC_crates/base/src/resizable/`、`input/base/state.rs` | § 4.8 每行已单独标注路径 |

### 7.3 本项目落地位置

| 范围 | 文件 |
|---|---|
| 主题初始化 / 字体 / S-15 深色投影 / S-16 文本封装 | `crates/ui/src/theme.rs` |
| 消息 / 工具卡 / thinking / minimap | `crates/ui/src/chat.rs` |
| 会话侧栏 | `crates/app/src/session_sidebar.rs` |
| composer / 面板 | `crates/app/src/panels.rs` |
| 窗口 / 工具栏 / dock | `crates/app/src/workspace.rs` |

---

## 8. 修订记录

已拆出至 [`UI设计规范-CHANGELOG.md`](UI设计规范-CHANGELOG.md)。

本文件只承载**生效条款**；「某个值为什么变成现在这样」「v2.0 当初写错了什么」一律去 CHANGELOG 查。
勘误内容不再回流正文——v2.1 与 v2.2 的勘误各有 1~4 处副本没改到（见 CHANGELOG v2.3 条目），
根因就是勘误与条款混排时，改一处容易漏其余。
