# BACKLOG

跨轮次发现的问题记这里，**不当场改**（CLAUDE.md 红线 3）。
每条注明：发现于哪一轮、影响哪一轮、严重度。

| # | 发现于 | 影响 | 严重度 | 内容 |
|---|---|---|---|---|
| 1 | R0 | R1 | 低 | ~~PowerShell 脚本无法在 130 验证~~ → 已装 pwsh 7.6.5，`validate.ps1` / `check-pins.ps1` 均已实跑 + 反向测试。R1 Windows 真跑发现 `fetch-pi.ps1` 解压失败：官方 zip 的 `pi.exe` 位于根目录，脚本却只检查 `vendor/pi/pi.exe`；spike 不依赖该文件，留待后续轮次按范围修复 |
| 2 | R0 | R1 | 低 | 已补测：真正链接 GPUI 的 `target/release/spike.exe` 约 20MB；原 `gpui-pi.exe` 仍约 1.4MB（自检程序未开窗口）。体积低于立项文档 50–90MB 估计，待正式入口和资源/内核打包后再复核 |
| 3 | R0 | R4 | 低 | R1 的 spike 代码允许脏，但必须在 R4 之前删除或重构进正式代码 |
