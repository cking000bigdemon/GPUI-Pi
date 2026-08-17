# BACKLOG

跨轮次发现的问题记这里，**不当场改**（CLAUDE.md 红线 3）。
每条注明：发现于哪一轮、影响哪一轮、严重度。

| # | 发现于 | 影响 | 严重度 | 内容 |
|---|---|---|---|---|
| 1 | R0 | R1 | 低 | ~~PowerShell 脚本无法在 130 验证~~ → 已装 pwsh 7.6.5，`validate.ps1` / `check-pins.ps1` 均已实跑 + 反向测试。R1 Windows 真跑发现 `fetch-pi.ps1` 解压失败：官方 zip 的 `pi.exe` 位于根目录，脚本却只检查 `vendor/pi/pi.exe`；spike 不依赖该文件，留待后续轮次按范围修复 → **2026-08-17 已修复**：解包到 vendor 下同卷临时目录，自动定位产物（zip 根内容或顶层目录）后原子发布到 `vendor/pi/`；实测重跑下载 + SHA256 + 发布 + 自检全绿 |
| 2 | R0 | R1 | 低 | 已补测：真正链接 GPUI 的 `target/release/spike.exe` 约 20MB；R4 正式入口 + 图标 + 1.2MB Noto Sans SC 字体后的 `target/release/gpui-pi.exe` 实测约 **19MB**。尚未包含 R17 的 `vendor/pi` 与安装包，最终体积留待 R17 复核 |
| 3 | R0 | R4 | 低 | ~~R1 的 spike 代码允许脏，但必须在 R4 之前删除或重构进正式代码~~ → R4 已删除 `spike` bin、benchmark 数据、冷启动辅助脚本及 `gpui-fps` / `instant` 直依赖；正式入口复用了经门禁验证的 GPUI 应用启动模式 |
| 4 | R7 | R7 | 中 | ~~同一 session 的默认展示应追平 pi-web 的“对话主线”：默认只展示用户 Query 与最终 Assistant Answer；thinking、工具调用/结果、子代理等过程消息统一收进折叠的“过程详情”，需要时再展开。~~ → R7 人工验收阶段纳入实现：右侧目录仅索引 Query / final Answer；已完成 turn 默认折叠过程，活跃 turn 展开；thinking 与工具详情均二级折叠、点击后惰性渲染。 |
| 5 | R8 | 后续所有 PR 的 linux 兜底 job | 低 | ~~`scripts/fetch-pi-web.sh` 在 git 中缺可执行位（`100644`，其余 `.sh` 均为 `100755`），ubuntu 非阻断 job 的 `./scripts/fetch-pi-web.sh` 直接 `Permission denied`（exit 126），自 PR #14 起每次复现；linux 是 `continue-on-error` 兜底，不影响合并。修复：`chmod +x scripts/fetch-pi-web.sh` 后随任一 PR 提交~~ → **2026-08-17 随 Windows solo 迁移关闭**：项目不再维护 Linux CI，`ubuntu-latest` job 已从 `.github/workflows/ci.yml` 移除，`.sh` 脚本不再被 CI 执行，本问题失去影响面 |
