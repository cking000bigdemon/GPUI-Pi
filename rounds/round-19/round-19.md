# Round 19 — Windows 应用图标

> 执行方：**Windows** · 状态：进行中（PR [#21](https://github.com/cking000bigdemon/GPUI-Pi/pull/21) 待审）

## 目标

将用户提供的黑色金属质感 `Pi` 图标作为 Windows `gpui-pi.exe` 的正式应用图标，使 Explorer、窗口类、任务栏和 Alt+Tab 均加载同一资源，且小尺寸渲染无白底、黑框、裁切或明显变形。

## 前置

- R0–R11 已完成并合并，基于 `main` 提交 `8163e32871ee833ba2149d7d8b628cbbceee5a3c`。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- 独立 worktree `D:\variFlight_work\GPUI-Pi-icon-preview` 已运行 `fetch-pi.ps1`、`fetch-pi-source.ps1`、`fetch-pi-web.ps1` 和 `check-pins.ps1`；`vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/` 完整且 pins 全绿。
- 原始图片由用户提供：`ChatGPT Image 2026年8月19日 11_49_43.png`，1254×1254 RGBA，SHA-256 `070dde7d877881913289d37025ea5825732e41a13245d7cd0f27f912a0dcb220`。

## 交付物

- `crates/app/assets/app-icon.ico`：16 / 20 / 24 / 32 / 40 / 48 / 64 / 128 / 256 共 9 档、32bpp 的 Windows ICO；SHA-256 `f6cff1404e0e73985ca675798a2f35f29cfd2edd02c0377234b1707cec7f26d9`。
- `crates/app/assets/app.rc`：以资源 ID `1` 声明应用图标，对应 GPUI Windows 后端的 `LoadImageW(..., PCWSTR(1), IMAGE_ICON, ...)` 契约。
- `crates/app/build.rs`：在 Windows MSVC 构建时定位 host 架构的 Windows SDK `rc.exe`，按 UTF-8 编译资源并仅链接到 `gpui-pi` bin；不修改 `Cargo.lock`。
- `rounds/round-19/round-19.md`、`ROUNDS.md`：本轮管理与验收记录。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 门禁与全量构建 | `.\scripts\validate.ps1` 全绿：pins、fmt、clippy `-D warnings`、workspace tests、release build |
| T2 | 资源契约 | release exe 可提取非空关联图标；GPUI 固定读取的 RT_GROUP_ICON ID `1` 存在；`Cargo.lock` 与上游 pins 不变 |
| T2 | 多尺寸资源 | ICO 包含 16–256 px 共 9 档，任务栏与 Alt+Tab 不依赖单一大图缩放 |
| T3 | 用户与视觉终审 | release 客户端实机启动；用户确认通过；任务栏和 Alt+Tab 截图经 `visual-reviewer` 的 `SCREENSHOT` 模式审查，结论必须为 `PASS` |

## 禁止

- 不直接修改或提交到 `main`；使用 `WinClaude/round-19` 分支和 PR。
- 不修改 `Cargo.lock`、`PINNED_PI_VERSION`、vendor 上游源码或 pins。
- 不引入 WebView、HTML 或新的运行时技术栈。
- 不改变现有窗口布局、Theme token、业务逻辑或会话数据。
- 不写入真实 `~/.pi`；截图证据只保存到仓库根 gitignored 的 `.pi/`。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-19/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 视觉审查

- 视觉审查模式：SCREENSHOT
- 视觉审查结论：PASS
- 截图验证：已提供（SCREENSHOT_PROVIDED）
- 兜底原因：N/A
- `requested_at`：`2026-08-19T12:52:43.6385957+08:00`
- `deadline`：`2026-08-19T13:02:43.6385957+08:00`
- 审查报告 / 证据：`.pi/visual-review/app-icon/evidence/manifest-2d2a86d502c5a2f1.json`；截图 `.pi/visual-review/app-icon/evidence/95a1bd48-01.png`（主窗口 + 任务栏）与 `95a1bd48-02.png`（Alt+Tab）。manifest 的 `actualImageCount=2`，`selectedEntries` 仅为本次回传的 `95a1bd48`。
- 说明：SCREENSHOT 已验证真实渲染。项目专用 `visual-reviewer` 判定任务栏与 Alt+Tab 均加载正确资源；未发现白底、黑框、异常 halo、明显锯齿、裁切、变形或错误图标。

## 本轮实测

- 启动门禁全绿：pi `0.84.2`、pi 源码 `v0.84.2 / 914cf147...`、pi-web `v0.8.9 / 2a6e537...` 及 gpui / gpui-component pins 均未漂移。
- 两次实现阶段完整 `.\scripts\validate.ps1` 均返回 `VALIDATE OK`；代码审查修复 `rc.exe` 应按 host 架构选择、`.rc` 路径不依赖 CWD、SDK 版本数值排序、旧 SDK 布局、`RC` 缓存失效与 UTF-8 代码页后，再次全绿。
- 测试数字：app `47 passed / 1 ignored`，UI `19 passed`，pi-data unit `38 passed`、integration `13 passed`，pi-render `30 passed`，pi-rpc `20 passed`；真实 pi / 付费模型测试继续按显式环境门禁 ignored。
- release `target/release/gpui-pi.exe` 可由 `System.Drawing.Icon::ExtractAssociatedIcon` 提取 `32×32` 关联图标；运行进程窗口标题为 `GPUI-Pi` 且响应正常。
- 用户在实际 release 客户端审核后明确回复“通过”，并在 10 分钟窗口内回传任务栏与 Alt+Tab 两张完整截图。
- 独立代码审查使用 `claude-code-review` 两轮完成：首轮 2 项 medium 已关闭；复审无 correctness / security 阻断项，仅余 MSVC-only 范围外及错误信息可读性的 low 建议。
- 图标由用户提供的原始 PNG 在本地用高质量缩放生成多尺寸 PNG-compressed ICO；未将 2.5 MiB 原始图片复制进仓库，来源文件名与 SHA-256 已在本任务卡留档。
- 提交 `2c83a35` 已推送至 `origin/WinClaude/round-19`，PR [#21](https://github.com/cking000bigdemon/GPUI-Pi/pull/21) 已创建；PR 描述包含最终 validation 实际回显摘要、测试数字、代码审查与截图视觉审查结论。
