# R3 脱敏会话 fixture

这些 JSONL 由本机真实 `~/.pi/agent/sessions` 读取后生成，只保留协议结构和数值字段。prompt、回复、工具输出、路径、URL、命令、标题、base64 与凭据均替换为 `<redacted>`；header id/cwd/timestamp 也已归一化。fixture 不用于内容快照，只用于文件层兼容性与不 panic 验收。
