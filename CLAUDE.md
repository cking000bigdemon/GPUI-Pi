# CLAUDE.md — GPUI-Pi 开发约定

> 权威设计文档是 [`docs/立项文档.md`](docs/立项文档.md)。本文件是**每轮必守的操作约定**，与它冲突时以立项文档为准。
> `AGENTS.md` 是本文件的镜像，改动请同时更新两份。

## 这是什么

pi 编程智能体的**原生桌面客户端**：GPUI + gpui-component 画界面，官方 pi 独立二进制（`pi --mode rpc`）当内核。
没有 Electron、没有 Chromium、没有 Next.js、没有内置 Node / Python。

## 五条红线

1. **不引入 web 技术栈** —— 不加 WebView（含 `gpui-wry`）、不嵌 HTML 页面。Mermaid 按立项文档 § 一 直出源码。
2. **不动上游钉版本** —— `Cargo.lock` 与 `PINNED_PI_VERSION` 是钉死点，`cargo update` 会被 `scripts/check-pins.*` 判红。要改版本先改立项文档 § 二。
3. **不跨轮次改动** —— 发现前面轮次的问题写进 `rounds/BACKLOG.md`，不当场顺手改。
4. **不放宽验收** —— 连续 2 次 validation 不过，写 `rounds/BLOCKED-NN.md` 停下呼人，禁止改标准让自己通过。
5. **不写 `~/.pi` 的破坏性操作** —— 数据目录与终端 pi、pi-web-desktop 共享。能只读就只读，必须写走「临时文件 + rename」。

## 每轮怎么跑

```
读 rounds/round-NN.md  →  实现  →  跑 validate  →  不过就改，重跑
                                        ↓ 全绿
                     commit + 更新 ROUNDS.md + 回填任务卡「本轮实测」
```

```bash
./scripts/validate.sh            # 全量（含 GPUI 编译，慢）
./scripts/validate.sh --logic    # 只跑 pi-rpc / pi-data / pi-render（快）
```

```powershell
.\scripts\validate.ps1
.\scripts\validate.ps1 -Logic
```

**任何一次迭代结束都必须跑 validate**，不许"看起来对了就下一步"。

## 平台归属

| 范围 | 执行方 |
|---|---|
| R0 工程骨架 | 130 Arch |
| **R1–R17 全部实现、测试、打包** | **Windows** |
| CI | `windows-latest` 阻断；`ubuntu-latest` 只跑纯逻辑 crate 且非阻断 |

从 Linux 交叉编译 Windows 的 GPUI 目标**不可行**（DirectX + COM + MSVC 链接器）。R1 起 130 端只做只读工作。

## crate 分层

| crate | 依赖 GPUI | 放什么 |
|---|---|---|
| `pi-rpc` | ❌ | 子进程、JSONL 协议、命令/事件类型 |
| `pi-data` | ❌ | `~/.pi/agent` 文件层 |
| `pi-render` | ❌ | 消息 → 可渲染中间模型（Markdown 分块 / ANSI / diff / 工具卡片） |
| `gpui-pi-ui` (`crates/ui`) | ✅ | 跨面板复用的组件封装 |
| `gpui-pi` (`crates/app`) | ✅ | 窗口、面板、状态编排、入口 |

**能放进前三个就不要放进后两个** —— 那三个不需要窗口和 GPU 就能全量单测，是自动化验收的全部基础。

## 编码约定

| 项 | 约定 |
|---|---|
| 换行 | 一律 LF（`.gitattributes` 已强制）。判断"文件是否被改过"的哈希必须忽略换行差异 |
| 路径 | 只用 `std::path::Path`，禁止手拼 `/` 或 `\`；长路径用 `\\?\` 或 `dunce` 规范化 |
| home 目录 | 走 `dirs` crate，别自己读 `HOME`（Windows 上是 `USERPROFILE`） |
| 子进程终止 | 统一走 `pi_rpc` 的封装：Windows `taskkill /T /PID`，其余 `kill(-pgid)`。只杀父进程会留僵尸 |
| 文件替换 | Windows 上被打开的文件不能删/改名 —— 写临时文件 + rename，rename 前确认句柄已关 |
| 颜色/字体 | 颜色一律走 gpui-component 的 `Theme` 变量，禁止硬编码；字体族按平台条件编译（Windows 微软雅黑/Segoe UI，其余 Noto Sans CJK SC） |
| 注释 | 中文。写**为什么**，不复述代码在做什么 |
| clippy | `-D warnings`，不许 `#[allow]` 糊过去；确有必要时必须写明理由 |
| PowerShell 脚本 | 改完**必须验证**，不许"照 sh 版翻译完就提交"（R0 因此红过一次 CI）。130 上已装 pwsh 7.6.5（`~/.local/bin/pwsh`）：能跑的直接跑，跑不了的至少过一遍语法解析（`[Parser]::ParseFile`） |
| `$LASTEXITCODE` | PowerShell 只有跑过**外部程序**才写它 —— 调 `.ps1` 且它正常结束时该变量保持旧值、首次调用时是空的。判断成败前先 `$global:LASTEXITCODE = 0`，且被调脚本成功时要显式 `exit 0` |

## 上游源码在哪（只读参考）

| 用途 | 位置 |
|---|---|
| 功能对照基线 pi-web 0.8.9 | `git clone --depth 1 https://github.com/agegr/pi-web`（钉 `2a6e5371`） |
| RPC 协议权威文档 | pi 发布包内 `docs/rpc.md`（1589 行），或 `vendor/pi/docs/` |
| 会话文件格式 | 同上 `docs/session-format.md` |
| 组件库用法 | `git clone --depth 1 https://github.com/longbridge/gpui-component`，看 `crates/story/src/stories/` |

## 协作

- worktree + PR，分支 `WinClaude/round-NN`（130 端用 `ArchLinuxClaude/round-NN`）；
- `main` 只接 PR，不直推；
- PR 描述必须贴 validation 的实际回显，不贴不审。
