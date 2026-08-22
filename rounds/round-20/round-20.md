# Round 20 — Release 试用第一批问题修复

> 执行方：Windows · 状态：✅ 已完成

## 目标

修复 [`docs/Release试用问题清单.md`](../../docs/Release试用问题清单.md) 当前记录的 UI-001 至 UI-008，使 Release 试用中的项目会话浏览、新建会话、Tool CTA 展开、右侧导航、图片输入与流式图片消息表现符合清单中的预期行为。

## 前置

- `main@e942edd` 已包含 Release 试用问题清单。
- Round 0–16、19 已完成，Round 14 已合并进 `main`。
- 新 worktree 已独立执行 `fetch-pi.ps1`、`fetch-pi-source.ps1`、`fetch-pi-web.ps1` 与 `check-pins.ps1`，完整 vendor 门禁全绿。
- 功能对照使用本 worktree 的 `vendor/upstream/pi-web-0.8.9/`；协议参考使用本 worktree 的 `vendor/upstream/pi-0.84.2/`。

## 交付物

- `crates/app/src/session_sidebar.rs`：项目折叠、单项目最近 8 条会话与独立滚动、新建会话入口及相关状态编排。
- `crates/app/src/panels.rs`、`crates/app/src/live_session.rs`、`crates/app/src/workspace.rs`：新会话、Tool CTA 锚点、composer 图片附件与剪贴板、流式消息状态等应用层修复。
- `crates/ui/src/chat.rs` 及必要的 `crates/ui/**`：右侧层级导航、可调宽度、图片消息稳定渲染及相关 UI 组件修复。
- 必要时修改 `crates/pi-render/**`、`crates/pi-data/**` 或 `crates/pi-rpc/**`，但仅限 UI-003、UI-007、UI-008 的真实数据/协议通路需要，禁止借机扩展范围。
- 对应自动化测试与本任务卡实测记录。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 钉版本、格式、lint、单测、Release 编译 | `./scripts/validate.ps1` 全绿；日常迭代可先用 `./scripts/validate.ps1 -Logic`，收口必须全量 |
| T2 | UI-001/002/003 | 项目可折叠；每项目默认最近 8 条且超出部分只在项目会话区滚动；当前会话保持可见；可从当前项目启动不继承历史上下文的空白新会话 |
| T2 | UI-004/005 | Tool CTA 展开/收起后被点击条目保持视觉锚点；右侧导航可拖动并受合理最小/最大宽度约束，按 pi-web 会话结构呈现层级与缩进 |
| T2 | UI-006/007 | 添加多张图片后文本输入仍有可用高度并可垂直滚动；`Ctrl+V` 图片添加附件、普通文本仍正常粘贴，失败有可理解提示 |
| T2 | UI-008 | 含图片用户消息从发送到流式完成始终渲染稳定图片预览/占位，任何阶段都不显示原始 JSON/Base64 |
| T2 | 回归测试 | 针对项目折叠/8 条上限、新会话隔离、CTA 锚点、导航宽度、长文本多附件、剪贴板格式优先级、流式图片消息添加机械断言或纯逻辑单测 |
| T3 | Release 人工试用 | 使用 `target/release/gpui-pi.exe` 按问题清单逐项复现，确认 UI-001 至 UI-008 均不再出现；视觉审查按项目流程执行 |

## 禁止

- 不进入 R17 安装包制作、签名、升级或发布流程。
- 不修改 `Cargo.lock`、`PINNED_PI_VERSION`、`vendor/upstream/**` 身份或版本。
- 不引入 WebView、HTML UI、Node/Python 运行时或其他 web 技术栈。
- 不处理 `rounds/BACKLOG.md` 中与本轮八项问题无关的历史问题。
- 不以放宽“最近 8 条”、滚动锚点、图片稳定渲染或视觉规范来换取验收通过。

## 失败处理

同一验收项经针对性整改后连续 2 次 validation 仍不过 → 写 `rounds/round-20/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 视觉审查

- 视觉审查模式：SCREENSHOT
- 视觉审查结论：PASS
- 截图验证：已提供并完成真实 Release 复验
- 兜底原因：N/A
- `requested_at`：2026-08-22T20:19:53+08:00
- `deadline`：2026-08-22T20:49:53+08:00
- 最终证据 manifest：`.pi/visual-review/round-20/evidence/manifest-1dfbd5be3881bfb5.json`
- 最终审查报告：`.pi/visual-review/round-20/visual-review-final-pass-2.md`
- 说明：首轮视觉审查的 composer 重叠/裁切、输入壳、同轴、用户图片、StatusBar 密度、项目标题和会话行 findings 均已整改；后续真实截图发现的附件状态重叠与第 9 条越界绘制也已关闭。最终截图确认每项目只显示前 8 条完整会话、第 9 条任何像素不可见，项目专用 `visual-reviewer` 给出 `SCREENSHOT / PASS`。

## 本轮实测

- 启动门禁：`fetch-pi.ps1`、`fetch-pi-source.ps1`、`fetch-pi-web.ps1`、`check-pins.ps1` 已在本 worktree 独立执行，全绿。
- UI-001/002：项目 header 使用常驻 chevron，可按 project key 折叠；用户明确选择方案 2，项目独立滚动区使用现有设计 token `max_h_128`，会话行保持规范 `p_2 + gap_1`，机械实测默认完整显示前 8 条、第 9 条在视口外；选中项只在 selection 变化时一次 reveal，普通重绘不会把用户滚动弹回；刷新会清理不存在项目的折叠/滚动状态且 render 不再深拷贝整棵会话树。
- UI-003：项目 header 增加“新建会话”，入口先同步 browsing cwd、清旧选中并走与历史会话一致的 trust 提示，再通过 Workspace 启动 `initial_session=None` 的 fresh pi RPC；只有 fresh spawn 成功后才替换旧上下文，composer/附件/分支/file index/popup 状态清空并按新 cwd 重建索引。RPC `get_state` 出现真实 session file 后更新内存文档、事件泵校准路径并刷新侧栏。
- UI-004：Tool CTA 展开/收起在 splice 前记录 `logical_scroll_top`，splice 后同步恢复原 `ListOffset`，不再依赖同周期耗尽的 defer/bounds 重测；用户原本贴 tail 时保持 tail，不在 tail 才 detach。focused GPUI 测试断言视觉锚点偏差不超过 1px，并有纯逻辑 splice offset 回归。
- UI-005：右侧导航改为 `h_resizable` + `resizable_panel`，展开初始 176px、范围 144–320px，折叠 panel 的 size/range 固定 32px（实际内容宽 28–32px），不会继承默认 100px 下限或展开尺寸；chat 根恢复 theme background/min-size。pi-render 生成 user/assistant/h1-h3 层级；同消息多个 heading 使用 ordinal 唯一 ElementId，强选中只落在首个主节点。
- UI-006/007：Textarea 改 `auto_grow(1, 8)` 并移除固定 76px wrapper，保留组件内滚动；附件条单行横向滚动、缩略图 56px。首轮 `SCREENSHOT` 人工试用发现真实场景 9 失败：Textarea 获得焦点时 `Ctrl+V` 被内层 `Paste` action 消费，外层 `chat-workspace.on_key_down` 收不到，图片无法进入附件。针对性修复改为在 Textarea 祖先节点用 `capture_action<Paste>` 先执行图片分流：成功图片或“失败图片且无文本”停止传播；纯文本或“图片全失败但含文本”继续传播到 Textarea，并保留既定提示；旧 Ctrl+V keydown 双路径已删除。PNG/JPEG/GIF/WebP 编码能力由自动化验证闭合，真实 Release 场景 9 仍须重新截图验证；BMP/CF_DIB 与 SVG 明确报错，其中 BMP 转 PNG 受不改 `Cargo.lock`/不新增依赖范围限制，已登记 `rounds/BACKLOG.md` #10，不能宣称 BMP 场景已闭合。
- UI-008：live renderer 复用静态 `parse_image`，图片始终为 `Block::Image`；canonical identity 将 nested/flat image 统一为 mime + 长度 + 稳定 hash，同一无 id start/end 不重复且快照不含 Base64。
- 针对性测试：BMP/SVG 错误、live 图片稳定性、minimap heading 层级、fresh config、composer 8 行上限、项目折叠入口、Tool 锚点均通过。
- `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1 -Logic`：全绿，`VALIDATE OK`。
- 全量 validation 第一轮发现 `ready_chat_folds_completed_process_trace` 因 `h_resizable` 改造丢失 `chat-window` selector；另 `timeout_oversize_and_malformed_json_are_bounded` 在并发全套中偶发失败。修复 selector 后两项 focused 均单独通过。
- 独立代码审查首轮 findings 已整改：minimap 折叠宽度、fresh 工具重启 cwd/session、file index、Tool 逻辑锚点、8 行滚动、trust、minimap 唯一 ID、fresh 校准路径、tail 与 status 深拷贝均已关闭并补 focused 回归。
- 第二次复审整改补齐：剪贴板成功/失败图片分区与 Office text+CF_DIB fallback；fresh draft key 内容迁移；reducer identity 与校准路径同步；工具重启优先真实已落盘 session 且保持项目 cwd；selected child 按顶层 session reveal；生产 helper 测试替换自证式源码断言；会话 tooltip 恢复 branch/token/cost；折叠项目不构造 rows；折叠 minimap 移出 resizable group，固定 32px 且无误导 drag handle。
- 第二次整改 focused tests 全绿，包括 clipboard 分区、draft 迁移、fresh reset、restart path、child reveal、calibration slot、minimap 唯一 ID/宽度、8 行滚动、Tool 锚点、live identity/image。
- 最终代码复审整改补齐：Tool CTA 在默认 `FollowMode::Normal` 但已贴底时不再错误 detach；剪贴板 PNG+BMP 部分成功但附件批量达到上限时保留真实数量错误；minimap 改为同 message 首节点承载选中，h2-only assistant 也可高亮，并以预建 ordinal map 消除逐节点全表扫描；fresh spawn 后移除重复 `refresh_metadata`。
- 最终 focused tests 全绿：`tool_detail_toggle_keeps_default_normal_list_attached_at_tail`、`clipboard_batch_rejection_preserves_limit_error_over_partial_warning`、`minimap_h2_only_assistant_selects_its_first_node`、`minimap_heading_element_ids_are_unique`。
- 最终 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1 -Logic`：全绿，`VALIDATE OK`。
- 最终 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1`：全绿；app `133 passed / 0 failed / 1 ignored`，UI `29 passed / 0 failed`，logic crates 全绿，Release build 完成，`VALIDATE OK`。
- 场景 9 针对性修复新增真实 action-dispatch GPUI 回归：`composer_paste_action_captures_png_as_attachment_without_changing_text` 在 Textarea focus + `ClipboardItem::new_image(PNG)` + dispatch `Paste` 后断言附件 +1 且文本不变；`composer_paste_action_propagates_plain_text_to_textarea` 断言纯文本仍由 Textarea 原生插入。`cargo test -p gpui-pi composer_paste_action_ -- --nocapture`：`2 passed / 0 failed`；`cargo test -p gpui-pi clipboard_ -- --nocapture`：`2 passed / 0 failed`。
- 场景 9 修复后 `cargo fmt --all -- --check`、`git diff --check`、Cargo.lock clean 与无 staged 文件检查全绿。
- 场景 9 修复后 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1 -Logic`：全绿，`VALIDATE OK`。
- 场景 9 修复后 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1`：全绿；app `135 passed / 0 failed / 1 ignored`，UI `29 passed / 0 failed`，logic crates 全绿，Release build 完成，`VALIDATE OK`。
- 用户首轮 `SCREENSHOT` 已证明场景 9 修复前真实失败；自动化验证已通过，但人工 Release 场景与视觉审查仍待主代理重新请求截图，不得填写 `PASS`。
- 首轮视觉 reviewer 结论 `FAIL`：composer 区域重叠/裁切、composer 与消息列不同轴且输入壳不规范、用户图片过大、StatusBar 超 3 片段、项目标题层级偏弱、会话行 padding/gap 不符合规范。
- 视觉纯 UI 修复：composer 通栏只保留顶边框/背景，内部使用 820px 居中内容列；附件、会话控制、Textarea 输入壳、操作行、extension StatusBar 各自独立占高。Textarea 继续 `auto_grow(1, 8)`，改 `appearance(false).bordered(false)`，外壳使用 `rounded_xl + border_1 + shadow_sm`，focus 只切换 `ring` 边框色。用户图片通过纯 UI role context 使用已登记 `max_w_56/max_h_56`（224px，不超过 240px），工具图片保持原布局。StatusBar 最多显示 2 个状态和 1 个能力片段，剩余状态汇总为“还有 N 项”，tooltip 保留完整文本。项目标题恢复 `text_sm`；会话行恢复 `p_2`，项目滚动 children 恢复 `gap_1`。
- 8 行规格冲突已按用户决策闭合：选择方案 2，将项目独立滚动区从 `max_h_96` 调整为现有设计 token `max_h_128`；规范 `p_2 + gap_1` 保持不变，512px viewport 可完整容纳前 8 条双行会话，第 9 条位于视口外。独立滚动、selected reveal、折叠逻辑和普通重绘不回弹均保持。
- 视觉修复 focused tests 全绿：composer 长文本/区域无重叠与内容列轴线、StatusBar 汇总、项目标题/规范会话行、用户图片上限；Ctrl+V 两条真实 action tests 保持全绿。
- 视觉修复后 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/validate.ps1 -Logic`：全绿，`VALIDATE OK`。
- 首次全量 validation 在 clippy 阶段发现 `render_block` 8 参数违反 `too_many_arguments`；收束纯 UI context 参数后重跑全量 validation 全绿：app `136 passed / 0 failed / 1 ignored`，UI `30 passed / 0 failed`，Release build 完成，`VALIDATE OK`。
- 视觉结论仍为“待复验”，不得填 `PASS`。
- 视觉代码复核的两项 medium 已补修：StatusBar 按“能力预留 1、存在 overflow 再预留 1、剩余额度给状态”分配总计 3 个文本节点，5 状态场景现在仅渲染 `item-0 + overflow + capability`，完整隐藏状态仍在 tooltip；composer 改为与 `message-column` 相同的外层 `w_full + px_4 + justify_center`、内层 `w_full + min_w_0 + max_w(820)`，移除内容列 `px_2`，输入壳、附件、工具与操作均沿同一列边缘。
- medium follow-up focused tests 全绿：StatusBar 精确断言 `item-1/item-2` 不存在且 selector 计数为 3；新增 640px/1000px 两档 composer 输入壳边缘与 S-13 消息列公式一致的机械测试；长文本 composer 与两条 Paste action 回归继续通过。
- medium follow-up validation：`validate.ps1 -Logic` 与全量 `validate.ps1` 均 `VALIDATE OK`；全量 app `137 passed / 0 failed / 1 ignored`，UI `30 passed / 0 failed`，Release build 完成。视觉结论仍待真实截图复验。
- 最终 code review high finding：此前 composer 仍按 `chat-workspace` 全宽居中，而消息列按 ChatWindow 内 `h_resizable` 扣除实时 minimap 宽度后的消息 pane 居中；默认、拖拽、折叠目录时都会产生水平偏移，旧测试使用空 minimap 且按 workspace 公式自证，无法覆盖真实结构。
- high finding 修复：`ChatWindow` 在真实 `chat-message-scroll` pane 的 `on_prepaint` 回传 `Bounds<Pixels>`；`ChatPanel` 同时记录 workspace bounds，仅在 bounds 实际变化时更新并 notify，避免每帧无变化循环。composer 通栏背景/顶边框继续全宽，`composer-column-outer` 使用消息 pane 相对 workspace 的真实 inset 与 width，再在内部沿用 `px_4 + max_w(820)`，因此 minimap 展开、折叠、拖拽或窗口 resize 后输入壳都会跟随消息 pane。
- 删除旧 workspace 公式盲区测试，新增真实 minimap document 的 640px/1000px GPUI 测试：确认 callback 已跨帧写入 pane/workspace bounds，直接比较 `chat.rs` 的 `message-column` 与 `panels.rs` 的 `composer-textarea-control` 左右边缘；随后真实切换为 32px 折叠目录并再次断言同轴。StatusBar、长文本垂直布局、Textarea 与两条 Paste action focused tests 保持全绿。
- high finding 修复后 `validate.ps1 -Logic` 与全量 `validate.ps1` 均 `VALIDATE OK`；全量 app `137 passed / 0 failed / 1 ignored`，UI `30 passed / 0 failed`，Release build 完成。视觉结论仍待真实截图复验。
- 用户选择方案 2 后完成侧栏尺寸收口：仅将项目会话独立滚动区改为 `max_h_128`，保留 `p_2 + gap_1`、独立滚动、selected reveal 与折叠逻辑。10 条会话 focused 测试精确断言 viewport 为 512px、前 8 条完整可见、第 9 条在外，并验证滚动中段后普通重绘不回弹；折叠和 selected child reveal focused tests 同步通过。
- 方案 2 收口后 `validate.ps1 -Logic` 与全量 `validate.ps1` 均 `VALIDATE OK`；全量 app `137 passed / 0 failed / 1 ignored`，UI `30 passed / 0 failed`，Release build 完成。原 `max_h_96`/约 6 行规格冲突已闭合，视觉结论仍待真实截图复验。
- 最新 `SCREENSHOT` review（`.pi/visual-review/round-20/visual-review-final-1.md`）仍为 `FAIL`，阻断三项：浅色 Release composer 壳透明表面与阴影叠成中灰糊块；`max_h_128` 会话 viewport 未真正裁切，第 9 行仍被绘制；项目标题仍用 `muted_foreground`，层级弱于“新建会话”。
- 三项纯 UI 修复：composer 输入壳显式使用 `cx.theme().background` 浅色表面，继续保持 `rounded_xl`、单层 `border_1`、单层 `shadow_sm`，焦点只切换 border token，Textarea 仍 `appearance(false).bordered(false)`；项目会话滚动 viewport 增加 `overflow_hidden` paint/hitbox 裁切，同时保留 `max_h_128 + p_2 + gap_1`、独立滚动与全部会话数据；项目标题改为 `foreground`，保留 `text_sm + font_semibold + truncate`。
- 新增/强化 GPUI tests：composer style helper 精确断言 background token、单边框、单阴影及 focused 仅换 border；10 条会话默认第 9 行点击不可命中，滚动后第 9 行完整进入 viewport 且可命中，普通重绘仍不回弹。既有同轴、长文本垂直布局、StatusBar ≤3、折叠/reveal 与两条 Paste action tests 全绿。
- 三项修复后 `validate.ps1 -Logic` 与全量 `validate.ps1` 均 `VALIDATE OK`；全量 app `138 passed / 0 failed / 1 ignored`，UI `30 passed / 0 failed`，Release build 完成。视觉结论仍须主代理重新截图复验，不得填写 `PASS`。
- 最新 `SCREENSHOT` review（`.pi/visual-review/round-20/visual-review-attachment-overlap.md`）发现 high：加入附件后 `composer-textarea-viewport` 的 `min_h_0` 允许该行在纵向压力下收缩，但内部 input shell 与下方 actions 均为 `flex_none`，导致壳体越过父 viewport 并覆盖操作行；输入壳新浅底与项目标题本轮截图已确认 `PASS`。
- 附件重叠最小修复：仅将 `composer-textarea-viewport` 改为 `flex_none` 并移除该层 `min_h_0`；输入壳的浅底、`rounded_xl + border_1 + shadow_sm + overflow_hidden`、Textarea `auto_grow(1, 8)`、附件尺寸、同轴和 Paste 路径均保持不变。
- 新增真实 production attachment strip GPUI 回归：在 1000×1000 窗口分别覆盖未选择历史会话的 `ChatStatus::Empty` 与普通 Ready chat；无附件、有 1 张有效 PNG 附件、清空附件三种状态均直接读取 attachment strip、viewport/control/actions bounds，断言附件完整位于输入上方、control/viewport 与 actions 至少保留 `gap_2`，清空后仍无重叠。既有 popup attachment、浅底壳、长文本、minimap 同轴、两条 Paste action 与 StatusBar tests 全绿。
- 附件重叠修复后 `validate.ps1 -Logic` 与全量 `validate.ps1` 均 `VALIDATE OK`；全量 app `139 passed / 0 failed / 1 ignored`，UI `30 passed / 0 failed`，Release build 完成。视觉结论仍待主代理重新截图复验。
- 最新 `SCREENSHOT` review（`.pi/visual-review/round-20/visual-review-final-pass.md`）确认 composer 附件布局、浅底输入壳、项目标题均已 `PASS`，唯一剩余 high 是项目默认 viewport 仍露出第 9 条标题。根因是单元素连续调用 `overflow_hidden().overflow_y_scroll()`，后者把 y overflow 覆盖回 Scroll，Windows Release 仍绘制越界后代。
- 严格裁切修复：项目会话区改为明确两层。外层 `project-session-viewport` 只负责 `max_h_128 + pb_1 + overflow_hidden`，总高度保持 512px、有效 clip 高度为现有 spacing token 计算出的 508px；内层 `project-session-scroll` 使用 `size_full + min_h_0`，只负责 `gap_1`、track_scroll、`overflow_y_scroll`、轴锁定、滚动条和全部 rows。未截断数据、未压缩 `p_2` 行高，selected reveal 与独立滚动保持。
- Sidebar GPUI fixture 改为接近真实截图的 CJK 长标题与两行 metadata。测试分别读取外层 viewport 和内层 scroll：断言外层 512px、内层有效 clip 508px，第 8 行 bottom 精确贴 clip bottom、第 9 行 top 不早于 clip bottom；点击第 9 行标题上沿候选点不可命中。滚动后第 9 行完整进入内层 clip 且可命中，普通重绘不回弹；折叠、selected child reveal 与 ready sidebar tests 全绿。
- 严格裁切修复后 `validate.ps1 -Logic` 与全量 `validate.ps1` 均 `VALIDATE OK`；全量 app `139 passed / 0 failed / 1 ignored`，UI `30 passed / 0 failed`，Release build 完成。
- 最终 Release 截图 `6fd1a661-01.png` 显示 `VariFlightWork` 项目恰好 8 条完整会话，随后直接进入下一项目，未出现第 9 条任何像素；结合此前附件、输入壳、minimap、同轴、项目标题和用户图片证据，项目专用视觉 reviewer 最终结论为 `SCREENSHOT / PASS`（`.pi/visual-review/round-20/visual-review-final-pass-2.md`）。
