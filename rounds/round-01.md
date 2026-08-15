# Round 01 — 风险门禁 spike ⚠️

> 执行方：**Windows** · 状态：未开始
>
> **这是全项目唯一的停项开关。四条门禁任一不过，写 `BLOCKED-01.md` 停项汇报，
> 回退到「pi 二进制瘦身 Electron 版」方案。禁止降低阈值继续。**

## 目标

用一个最小 demo 窗口证明：GPUI 能扛住 GPUI-Pi 最吃紧的四件事。

## 前置

- R0 已合并
- Visual Studio Build Tools + Windows SDK
- `rustup toolchain install 1.97.1-x86_64-pc-windows-msvc`
- `.\scripts\fetch-pi.ps1` 已拉下 `vendor\pi\pi.exe`
- `git config --global core.longpaths true` + 注册表 `LongPathsEnabled = 1`
- 装好微软拼音**和**搜狗输入法

## 交付物

`crates/app` 下一个 `spike` bin（或 `--spike` 子命令），只含：

1. 一个多行输入框（gpui-component `textarea`）；
2. 一个 Markdown 视图，可被程序按固定节奏灌入文本；
3. 一个"灌 8000 token 长文"的按钮，30ms 一 chunk；
4. `gpui-fps` 帧率显示；
5. 5 条示例消息用于框选测试。

spike 代码**允许脏**，但必须在 R4 之前删掉或重构进正式代码 —— 记进 `BACKLOG.md`。

## 验收（四条全过）

| # | 门禁 | 判据 | 怎么测 |
|---|---|---|---|
| 1 | **中文 IME** | 微软拼音 / 搜狗在多行输入框中：候选窗定位正确、组合期不丢字、`Enter` 只提交候选**不误发消息** | 人工，两种输入法各连打 200 字，含中英混排与标点 |
| 2 | **流式渲染** | 8000 token 按 30ms/chunk 灌入 Markdown 视图，**帧率 ≥ 50fps** | `gpui-fps` 读数，取灌注期间最低值 |
| 3 | **文本选中** | 跨 5 条消息框选复制，剪贴板内容格式正确（换行、代码块不错乱） | 人工，粘贴到记事本核对 |
| 4 | **冷启动** | 从双击到窗口可交互 **< 1.5s** | 连测 5 次取中位数 |

T1 照常：`.\scripts\validate.ps1` 全绿。

## 已知风险与预案

| 风险 | 预案 |
|---|---|
| 门禁 2 不过（`TextView` 每次 delta 全量重解析） | 先试「已定稿段落缓存 + 只重解析尾段」的分块渲染；仍不过才算门禁失败 |
| 门禁 1 不过 | **应用层无预案** —— IME 在 GPUI 平台层。但它是 zed monorepo 的 in-tree 官方 crate（不是社区分支），所以是「停项 + 给上游提 issue，修复后重估」，不是永久放弃。**不许在应用层写 hack 绕过去** |

### 门禁 1 debug 时看哪里

GPUI 的 Windows IME 走 **IMM32 而非 TSF**，实现全在 `crates/gpui_windows/src/events.rs`（钉死 commit `cc053a4a` 已核对）：

| 症状 | 对应实现 |
|---|---|
| 候选窗位置不对 | `update_ime_position()` → `ImmSetCompositionWindow` + `ImmSetCandidateWindow` |
| 组合期丢字 / 串字 | `handle_ime_composition()` → `GCS_COMPSTR` / `GCS_RESULTSTR` 解析 |
| 输入框失焦后仍吃键 | `update_ime_enabled()` → `ImmAssociateContextEx` |
| `Enter` 误发消息（候选未提交就上屏） | `ImmNotifyIME(NI_COMPOSITIONSTR, CPS_COMPLETE)` 的时机 |

提 issue 时带上这几个函数名，比只说"中文输入有问题"有效得多。

## 禁止

- 不搭正式界面框架（那是 R4）；
- 不接 `pi --mode rpc`（那是 R2/R7），长文用本地假数据；
- 不为了过门禁而改判据。

## 失败处理

见文件头。任一门禁不过 → `rounds/BLOCKED-01.md` 写清哪条、实测数字、试过的预案，停下呼人。

## 本轮实测

<!-- 完成后回填：四条门禁的实际数字 -->
