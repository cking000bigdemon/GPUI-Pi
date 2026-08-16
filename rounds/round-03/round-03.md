# Round 03 — `pi-data`：`~/.pi/agent` 文件层

> 执行方：**Windows** · 状态：✅ 已完成

## 目标

让 `pi-data` 在不依赖 GPUI、且不破坏共享 `~/.pi/agent` 的前提下，可靠解析真实 pi v3 会话，安全往返 `models.json` / `settings.json` / `trust.json`，扫描含 `.ts.disabled` 的扩展，并把 linked worktree 会话归并到主仓库项目。

## 前置

- R0–R2 已完成并合并；`main` 位于 PR #5 合并结果。
- Windows 11 + Rust `1.97.1-x86_64-pc-windows-msvc`。
- 协议权威来源：钉死 `pi 0.84.2` 发布包内 `docs/session-format.md` 及对应 `SessionManager` 实现。
- 功能对照来源：钉死 `pi-web 0.8.9` 的 `lib/session-reader.ts`、`lib/project-identity.ts`、`lib/worktree.ts`。

## 交付物

- `crates/pi-data/src/session.rs`：流式 JSONL 解析、会话 header/entry/message 类型、列表摘要、损坏行隔离与 v1/v2/v3 兼容读取。
- `crates/pi-data/src/config.rs`：`models.json` / `settings.json` / `trust.json` 的保真 JSON 读写与同目录临时文件 + rename 原子替换。
- `crates/pi-data/src/extensions.rs`：扫描 `*.ts`、目录 `index.ts` 与 `*.ts.disabled`，同 id 下 enabled 版本优先。
- `crates/pi-data/src/project.rs`：路径 identity、git project 解析、linked worktree 归并、项目分组。
- `crates/pi-data/src/lib.rs`、`crates/pi-data/Cargo.toml`：公开 API 与依赖接线。
- `crates/pi-data/tests/fixtures/sessions/`：从真实 `~/.pi/agent/sessions` 脱敏生成的至少 20 个会话 fixture，不含 prompt 正文、密钥或用户绝对路径。
- `crates/pi-data/tests/real_snapshot.rs`：fixture 全量不 panic、摘要/分组与配置/扩展行为测试。
- 本任务卡：完成后回填实际命令、数字、踩坑与设计偏离。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 格式、clippy、单测、release 构建 | `./scripts/validate.sh --logic` 与 `./scripts/validate.sh` 均输出 `VALIDATE OK` |
| T2 | 真实脱敏快照 | `cargo test -p pi-data --test real_snapshot -- --nocapture`；至少 20 个真实来源 fixture 全部解析，不 panic，且不会访问真实 `~/.pi` |
| T2 | 会话格式 | 覆盖 v1/v2/v3、全部文档 entry 类型、string/array content、CRLF/无尾 LF、损坏行隔离、父会话路径映射、最新 session name 与消息摘要 |
| T2 | 配置安全写 | models/settings/trust 在临时目录往返一致；写入只使用目标目录中的唯一临时文件并 rename，失败不破坏旧文件、不残留临时文件 |
| T2 | 扩展与项目归并 | `*.ts` / `*.ts.disabled` / 子目录 `index.ts` 扫描正确；真实临时 git linked worktree 的会话与主 checkout 归入同一 project key |
| T3 | 无 | 本轮为纯逻辑 crate，不需要目视验收 |

## 禁止

- 不接 GPUI，不修改 app/ui 正式界面（R4+）。
- 不实现会话重命名、删除、导出或 trust UI（R5/R12）。
- 不实现 provider 发现、登录或模型连通性测试（R15）。
- 不修改真实 `~/.pi/agent`；真实目录只读，所有写测试使用临时目录。
- 不把真实 prompt、base64 图片、API Key、用户名或绝对路径提交进 fixture。
- 不改钉死的上游版本，不运行 `cargo update`。
- 不顺手修复 BACKLOG 中属于其他轮次的问题。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/round-03/BLOCKED.md`，停下呼人。禁止放宽验收标准自我通过。

## 本轮实测

- `pi-data` 新增四个纯逻辑模块：会话 JSONL、配置原子写、extension 扫描、project/worktree identity；不依赖 GPUI。
- 会话读取对 v1 缺失树字段、v2/v3 和未来未知 entry 宽容；已知的 8 种非 message entry 均有静态单测，message payload 保留原始 `serde_json::Value`，避免数据层抢做 R6 的渲染归一化。
- 会话列表只识别 pi 默认 `<project-dir>/<session>.jsonl` 布局，明确排除子代理目录中的 `run-N/session.jsonl`；父子会话路径会映射为 `parent_session_id`。
- 从真实 `~/.pi/agent/sessions` 生成并提交 **24 个**结构脱敏 fixture（约 865 KiB）：prompt、回复、工具输出、URL、路径、模型/provider 名、base64 和凭据已替换，敏感 marker 扫描无命中。
- 真实只读 T2：`PI_DATA_TEST_REAL_AGENT_DIR=C:/Users/.../.pi/agent cargo test -p pi-data --test real_agent_readonly -- --nocapture` 扫描 **172 个**顶层真实会话、0 diagnostic；测试前后真实 `settings.json` SHA256 都是 `8bfc8c3f…`。
- 配置层以 `serde_json::Value` 保留未知字段；同目录 `create_new` 临时文件写入后 `flush + sync_all`，关闭句柄后替换。Windows 使用 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`，不会先删除共享配置。
- extension 扫描覆盖顶层 `*.ts` / `*.ts.disabled`、子目录 `index.ts` / `index.ts.disabled`，enabled 与 disabled 同时存在时 enabled 优先，并跳过 `node_modules` / 备份目录。
- project identity 在 Windows 下归一分隔符、大小写和尾分隔符；真实临时 git linked worktree 黑盒测试验证 main checkout 与 linked checkout 归并到同一 project key，而 repo 子目录保持独立 identity。
- `pi-data` 最终为 **13 个单元测试 + 5 个 fixture/worktree 集成测试 + 1 个 opt-in 真实只读测试**；`cargo test -p pi-data` 全绿。
- 最终 validation：`./scripts/validate.sh --logic` 和 `./scripts/validate.sh` 均输出 `VALIDATE OK`；全量 validation 包含 GPUI clippy/test/release build。
- PR：[#6](https://github.com/cking000bigdemon/GPUI-Pi/pull/6)；CI 结论见本轮后续文档提交。
- 审查说明：按约定先尝试 `deepseek/deepseek-v4-pro` reviewer，再回退继承模型；本机 subagent runtime 两次 reviewer 都在 0 tool 状态停滞，未产出审查报告。主会话因此完成逐文件审查，并修正了两项：过滤嵌套 subagent session，及 Windows `canonicalize` 的 `\\?\\` 前缀导致的 worktree identity 风险（改用 `dunce`）。
- PowerShell 说明：本机只有 Windows PowerShell 5.1，它把仓库 UTF-8 无 BOM 中文脚本误解码并 parser error；没有修改脚本。Git Bash validation 的同一 cargo 门禁均已全绿。
