# BACKLOG

跨轮次发现的问题记这里，**不当场改**（CLAUDE.md 红线 3）。
每条注明：发现于哪一轮、影响哪一轮、严重度。

| # | 发现于 | 影响 | 严重度 | 内容 |
|---|---|---|---|---|
| 1 | R0 | R1 | 低 | ~~PowerShell 脚本无法在 130 验证~~ → 已装 pwsh 7.6.5，`validate.ps1` / `check-pins.ps1` 均已实跑 + 反向测试。R1 Windows 真跑发现 `fetch-pi.ps1` 解压失败：官方 zip 的 `pi.exe` 位于根目录，脚本却只检查 `vendor/pi/pi.exe`；spike 不依赖该文件，留待后续轮次按范围修复 → **2026-08-17 已修复**：解包到 vendor 下同卷临时目录，自动定位产物（zip 根内容或顶层目录）后原子发布到 `vendor/pi/`；实测重跑下载 + SHA256 + 发布 + 自检全绿 |
| 2 | R0 | R1 | 低 | 已补测：真正链接 GPUI 的 `target/release/spike.exe` 约 20MB；R4 正式入口 + 图标 + 1.2MB Noto Sans SC 字体后的 `target/release/gpui-pi.exe` 实测约 **19MB**。尚未包含 R17 的 `vendor/pi` 与安装包，最终体积留待 R17 复核 |
| 3 | R0 | R4 | 低 | ~~R1 的 spike 代码允许脏，但必须在 R4 之前删除或重构进正式代码~~ → R4 已删除 `spike` bin、benchmark 数据、冷启动辅助脚本及 `gpui-fps` / `instant` 直依赖；正式入口复用了经门禁验证的 GPUI 应用启动模式 |
| 4 | R7 | R7 | 中 | ~~同一 session 的默认展示应追平 pi-web 的“对话主线”：默认只展示用户 Query 与最终 Assistant Answer；thinking、工具调用/结果、子代理等过程消息统一收进折叠的“过程详情”，需要时再展开。~~ → R7 人工验收阶段纳入实现：右侧目录仅索引 Query / final Answer；已完成 turn 默认折叠过程，活跃 turn 展开；thinking 与工具详情均二级折叠、点击后惰性渲染。 |
| 6 | R9 | R9 起每一轮的 T1 | 中 | `scripts\validate.ps1` 没有 UTF-8 BOM（同目录的 `check-pins.ps1` 有）。Windows PowerShell 5.1 对无 BOM 的 `.ps1` 按系统 ANSI（简中机器上是 GBK）解码，脚本里的中文注释被拆成非法字节序列，**整个文件解析失败**（报 `TerminatorExpectedAtEndOfString` + `Missing closing '}'`），一行都跑不到。本机只有 WinPS 5.1，BACKLOG #1 提到的 pwsh 7 装在已退役的 130 Arch 机上，Windows solo 后不存在。R9 因此改为**按 `validate.ps1` 的定义逐条执行同样的五步**（check-pins / fmt --check / clippy -D warnings / test / build --release），验收内容未打折。修复（属 R0 脚本，本轮不动）：给 `validate.ps1` 补 UTF-8 BOM，或改用全 ASCII 注释；`fetch-pi*.ps1`、`check-pi-*-pin.ps1` 同样无 BOM，目前只是输出中文变乱码、尚能解析，建议一并补 |
| 5 | R8 | 后续所有 PR 的 linux 兜底 job | 低 | ~~`scripts/fetch-pi-web.sh` 在 git 中缺可执行位（`100644`，其余 `.sh` 均为 `100755`），ubuntu 非阻断 job 的 `./scripts/fetch-pi-web.sh` 直接 `Permission denied`（exit 126），自 PR #14 起每次复现；linux 是 `continue-on-error` 兜底，不影响合并。修复：`chmod +x scripts/fetch-pi-web.sh` 后随任一 PR 提交~~ → **2026-08-17 随 Windows solo 迁移关闭**：项目不再维护 Linux CI，`ubuntu-latest` job 已从 `.github/workflows/ci.yml` 移除，`.sh` 脚本不再被 CI 执行，本问题失去影响面 |
| 7 | R12 | 后续会话删除 UI 复核 | 低 | 普通 `Dialog` 仅配置 `button_props` 可能不显示会话删除 footer，待后续轮次验证并修复；R12 按红线 3 不修改前序会话删除代码。 |
| 8 | R15 | 项目命令环境与 extension 优先级 | 中 | pi 0.84.2 将 CLI `-e` extension 排在自动发现 extension 前，API 又不能枚举后续 `user_bash` handlers。R15 已在所有 `session_start` 完成后的 `resources_discover` 复检 owner，可避让加载期及 async `session_start` 内注册的 bash；但默认工具集下，后加载且**只注册 `user_bash`、完全不注册 bash tool** 的用户扩展仍无法被 host 可靠检测，且在 `resources_discover`/host 注册之后才动态注册同名 bash tool 的 extension 仍会被排首位的 CLI host 遮蔽。GPUI-Pi 与 pi-web inline host ordering 因而仍有残留偏差；彻底解决需要上游提供 extension ordering/handler introspection，或 RPC 原生项目命令环境。另记录：None/ReadOnly 无用户 handler 时 direct RPC bash 回退 pi 原生 operations、不做项目环境清洗；GPUI 原生宿主在 host 生效时按 pi-web 规则删除继承的 `PORT`/`NODE_ENV`/`NEXT_*`，相对终端 pi 有意存在差异；旧 temp 版本清理与 POSIX `/tmp` 多用户权限可在打包/跨平台范围复核；POSIX 共享 `/tmp` 下还存在“落盘校验后、pi 实际 import 前文件被替换”的 TOCTOU 任意代码执行风险，若恢复非 Windows 支持必须改到用户私有目录并设 0700 权限。 |
