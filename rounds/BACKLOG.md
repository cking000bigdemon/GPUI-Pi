# BACKLOG

跨轮次发现的问题记这里，**不当场改**（CLAUDE.md 红线 3）。
每条注明：发现于哪一轮、影响哪一轮、严重度。

| # | 发现于 | 影响 | 严重度 | 内容 |
|---|---|---|---|---|
| 1 | R0 | R1 | 低 | ~~PowerShell 脚本无法在 130 验证~~ → 已装 pwsh 7.6.5，`validate.ps1` / `check-pins.ps1` 均已实跑 + 反向测试。**只剩 `fetch-pi.ps1` 仅过语法解析**（依赖 `PROCESSOR_ARCHITECTURE` 与 Windows zip），R1 在 Windows 上真跑一次即可闭环 |
| 2 | R0 | R1 | 低 | `target/release/gpui-pi` 只有 1.9MB —— app 尚未真正调用 GPUI，链接器裁掉了渲染栈。立项文档 50–90MB 的体积估计**尚未验证**，R1 开出窗口后补测 |
| 3 | R0 | R4 | 低 | R1 的 spike 代码允许脏，但必须在 R4 之前删除或重构进正式代码 |
