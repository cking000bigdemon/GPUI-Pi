# BACKLOG

跨轮次发现的问题记这里，**不当场改**（CLAUDE.md 红线 3）。
每条注明：发现于哪一轮、影响哪一轮、严重度。

| # | 发现于 | 影响 | 严重度 | 内容 |
|---|---|---|---|---|
| 1 | R0 | R1 | 低 | ~~PowerShell 脚本无法在 130 验证~~ → 已装 pwsh 7.6.5，`validate.ps1` / `check-pins.ps1` 均已实跑 + 反向测试。R1 Windows 真跑发现 `fetch-pi.ps1` 解压失败：官方 zip 的 `pi.exe` 位于根目录，脚本却只检查 `vendor/pi/pi.exe`；spike 不依赖该文件，留待后续轮次按范围修复 → **2026-08-17 已修复**：解包到 vendor 下同卷临时目录，自动定位产物（zip 根内容或顶层目录）后原子发布到 `vendor/pi/`；实测重跑下载 + SHA256 + 发布 + 自检全绿 |
| 2 | R0 | R1 | 低 | 已补测：真正链接 GPUI 的 `target/release/spike.exe` 约 20MB；R4 正式入口 + 图标 + 1.2MB Noto Sans SC 字体后的 `target/release/gpui-pi.exe` 实测约 **19MB**。尚未包含 R16 的 `vendor/pi` 与安装包，最终体积留待 R16 复核 |
| 3 | R0 | R4 | 低 | ~~R1 的 spike 代码允许脏，但必须在 R4 之前删除或重构进正式代码~~ → R4 已删除 `spike` bin、benchmark 数据、冷启动辅助脚本及 `gpui-fps` / `instant` 直依赖；正式入口复用了经门禁验证的 GPUI 应用启动模式 |
