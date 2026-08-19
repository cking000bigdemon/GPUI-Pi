<!-- 本文件由 CLAUDE.md 镜像而来，改动请同步两份。 -->

# CLAUDE.md — GPUI-Pi 开发约定

> 权威设计文档是 [`docs/立项文档.md`](docs/立项文档.md)。本文件是**每轮必守的操作约定**，与它冲突时以立项文档为准。
> `AGENTS.md` 是本文件的镜像，改动请同时更新两份。

## 这是什么

pi 编程智能体的**原生桌面客户端**：GPUI + gpui-component 画界面，官方 pi 独立二进制（`pi --mode rpc`）当内核。
没有 Electron、没有 Chromium、没有 Next.js、没有内置 Node / Python。

## 六条红线

1. **不引入 web 技术栈** —— 不加 WebView（含 `gpui-wry`）、不嵌 HTML 页面。Mermaid 按立项文档 § 一 直出源码。
2. **不动上游钉版本** —— `Cargo.lock`、`PINNED_PI_VERSION` 与 `vendor/upstream/` 下两份上游源码是钉死点，版本号与 commit 身份以「上游源码在哪」一表为准；`cargo update` 或源码身份漂移会被 `scripts/check-pins.ps1` 判红（`.sh` 版是历史遗留，不作为判红依据）。要改版本先改立项文档 § 二。
3. **不跨轮次改动** —— 发现前面轮次的问题写进 `rounds/BACKLOG.md`，不当场顺手改。
4. **不放宽验收** —— 同一验收项经针对性整改后连续 2 次 validation 仍不过，写 `rounds/round-NN/BLOCKED.md` 停下呼人，禁止改标准让自己通过；日常迭代中的其他失败按流程图「不过就改，重跑」处理，不触发此条。
5. **不写 `~/.pi` 的破坏性操作** —— 指用户主目录下的 pi 数据目录（与仓库根 gitignored 的 `.pi/` 日志目录同名异物，勿混淆），它与终端 pi、pi-web-desktop 共享。能只读就只读，必须写走「临时文件 + rename」。
6. **不把共享目录链接进 worktree** —— 从创建 worktree 起，禁止在其中建立任何指向主 checkout、其他 worktree 或外部共享目录的 Junction、目录 symlink 或其他 reparse point，尤其禁止链接 `vendor/`、`target/`、`.venv/`、仓库 `.pi/` 与用户数据目录 `~/.pi`。执行 `git worktree remove` 前必须检查 reparse point；发现任何目录链接就立即停止并呼人，禁止假设删除 worktree 只会删除链接本身——Git for Windows 可能沿 Junction 递归删除共享目标内容。目录如何独立准备见「新 round 启动门禁」。

## 新 round 启动门禁

`git worktree add` 完成后，**第一项操作就是在新 worktree 内独立准备并验证完整 `vendor`**；此门禁必须早于读取轮次任务卡、搜索上游对照源码、启动子代理或修改任何代码：

```powershell
.\scripts\fetch-pi.ps1
.\scripts\fetch-pi-source.ps1
.\scripts\fetch-pi-web.ps1
.\scripts\check-pins.ps1
```

开始实现前必须同时确认 `vendor/pi/pi.exe`、`vendor/upstream/pi-0.84.2/`、`vendor/upstream/pi-web-0.8.9/` 均存在，且 `check-pins` 全绿。每个 worktree 的本地目录必须独立准备，上游参考源码走本 worktree 自己的 fetch 脚本。任何一项缺失或准备失败都立即停止并呼人；禁止先寻找替代源码路径、边开发边补、创建共享目录链接（红线 6），或读取主 checkout / 其他 worktree 的 `vendor/` 顶替。临时只读参考可以直接读外部绝对路径，但不得挂载进 worktree，且这一口径只适用于 `vendor/` 之外的资源（如 gpui-component clone）——上游对照在门禁通过前后都只走本 worktree 自己的 `vendor/`。这样每轮从第一分钟起就有完整运行时和钉死对照源码，不再把实现时间浪费在寻找缺失的 `vendor`。

## 每轮怎么跑

```
新 worktree 独立准备完整 vendor + check-pins
                  ↓ 全绿
读 rounds/round-NN/round-NN.md  →  实现  →  跑 validate  →  不过就改，重跑
                                                 ↓ 全绿
        代码审查（按「代码审查工具路由」）→ 涉及 UI 则视觉审查，须 PASS：
              主代理出「截图请求清单」（页面路由 + 截图要求）→ 用户截图回传
              → 运行 visual-reviewer
                                                 ↓ 审查门禁通过
                    commit + 更新仓库根 ROUNDS.md + 回填任务卡「本轮实测」
```

```powershell
.\scripts\validate.ps1              # 全量（含 GPUI 编译，慢）
.\scripts\validate.ps1 -Logic       # 只跑 pi-rpc / pi-data / pi-render（快）
```

**任何一次迭代结束都必须跑 validate**，不许"看起来对了就下一步"。

## 轮次目录

- `rounds/` 根目录只放全局文件：`README.md`、`TEMPLATE.md`、`BACKLOG.md`；轮次总览 `ROUNDS.md` 在仓库根目录而非 `rounds/` 下，每轮收口时更新；
- 每轮建立独立目录 `rounds/round-NN/`，任务卡固定为 `rounds/round-NN/round-NN.md`；
- 属于该轮的管理产出（实测记录、阻塞报告、复盘、可提交的验收附件说明等）全部放在该目录下；阻塞报告固定命名为 `BLOCKED.md`；
- 源码、脚本、fixture、构建产物仍放各自的标准目录，不为了“归档”复制进 `rounds/`；本地大日志与截图继续放仓库根 gitignored 的 `.pi/`（与红线 5 的用户数据目录 `~/.pi` 无关），在任务卡中记录结论或引用路径；
- 新增或引用轮次文件时禁止恢复 `rounds/round-NN.md`、`rounds/BLOCKED-NN.md` 这类扁平路径。

## 代码审查工具路由

- **本节只约束已有代码改动的独立审查通道**：不规定实现阶段由主会话还是子代理承担，也不得据此改变模型基于任务、能力与当前上下文自主选择的任务拆解、委派策略或 writer。使用 `claude-code-review` 不代表主会话必须充当 writer，也不降低 worker、scout 等子代理在实现阶段的可用性或优先级。
- 当已有代码改动需要独立代码审查时，按开发 harness 选择审查通道：
  - 使用 **pi harness** 时：优先调用 `claude-code-review`（独立 Claude Code 子进程，只读审查，默认 `scope=working`）；若 `claude-code-review` 不可用（未登记、鉴权/限流不可用、启动失败等），降级派发 pi-subagents 的 `reviewer` 子代理，走下方「pi-subagents 审查路径」；
  - 使用 **Claude Code / codex** 时：**不适用上述路由规则**，代码审查按对应 harness 自身的流程执行。
- 审查器始终只读，并与当前 writer 隔离。主会话负责判断是否接受审查结论；需要修改代码时，保持审查前已经形成的 writer 归属：此前由主会话实现则由主会话修复，此前由 worker 实现则将 findings 交回同一 worker。审查工具的选择本身不得触发 writer 身份切换。

### pi-subagents 审查路径（pi harness 降级用）

- 启动 `reviewer` 前先用 `subagent action=models`（或 `/subagents-models reviewer`）核对当前运行时映射；
- 代码审查优先显式指定 **DeepSeek provider** 的 `deepseek/deepseek-v4-pro`；这里的 provider 必须是 `deepseek`，不得改用 `variflight-ticket`、OpenRouter 等其他 provider 的同名/路由模型；
- 若 `deepseek/deepseek-v4-pro` 未登记、鉴权/限流不可用或启动失败，立即以**不传 `model`** 的方式重试 reviewer，使其继承主会话模型；禁止拿 DeepSeek V4 Flash 或其他 DeepSeek 型号冒充 V4 Pro；
- **DSV4 Pro 的 reviewer 任务必须从第一个字符开始用简体中文**，并在任务首部明确写“严禁英文前导语，调用工具前只能写中文”。它有时会先输出 `I'll review` / `Let me`，触发桌面端 `language-guard` 中断；此时工具层常只显示误导性的 `Operation aborted`；
- DSV4 Pro 出现 `Operation aborted` 时，先查子代理 transcript 是否含 `[language-guard-restart]`。若存在，这是语言守卫中断，**不算模型/provider 启动失败，不得直接 fallback**；应保留 `deepseek/deepseek-v4-pro`，补强中文首字符约束后以 `context: fresh`、显式目标 `cwd` 重新运行，直到拿到完整审查结论；只有 transcript 无该标记且明确显示模型未登记、鉴权/限流或启动错误时，才按上一条 fallback；
- 模型优先级不改变审查隔离要求：审查默认 `context: fresh`、只读、不与 writer 共用写权限。

## 视觉还原度审查

- 项目专用视觉 reviewer 为 `.agents/visual-reviewer.md`（运行时名称 `visual-reviewer`）。它是**能力层只读** agent，只允许 `read` / `grep` / `find` / `ls`，禁止 `bash`、`edit`、`write`、子代理和任何文件修改；不得用通用代码 reviewer 替代视觉 reviewer。
- **触发条件**：本轮 diff 只要触及 `crates/ui/**`，或触及 `crates/app/**` 中的视图/组件/布局/样式渲染代码、Theme token、图标及其他视觉资源，就视为涉及 UI 层；即使改动位于其他路径，只要会改变用户可见的 UI 表现，也必须触发视觉 review。不得仅凭“没有改 `crates/ui`”而跳过。
- 视觉 review 是代码 review 之后的独立门禁：普通代码 review 的阻断 findings 已关闭、主会话判定代码 review 通过后，才运行 `visual-reviewer`；视觉 review 不能替代代码 review，也不得提前与尚未稳定的实现并行给出终审结论。
- 发起视觉 review 前，主会话或当前 writer 必须准备可比较证据：目标截图/设计稿或明确规范基线、对应窗口尺寸与缩放、主题、交互状态、当前 diff、任务卡，以及 `docs/UI设计规范.md`；其中**当前实现截图一律走用户回传**——主代理无法直接操作 GUI 应用（GPUI 桌面客户端）导航与截图，禁止自行臆造或从旧轮次/上游实现借用。主代理先产出「截图请求清单」，逐页给出：页面路由/导航路径（启动方式、窗口布局、到达该视图的操作序列、所需前置状态如会话内容/消息类型/工具调用等）、窗口尺寸与缩放、主题、交互状态（默认/hover/选中/错误/空态等）及该页对应的目标基线引用；随后请用户按清单操作应用并截图回传，主代理将截图保存到仓库根 gitignored 的 `.pi/` 下（按轮次分目录，路径随任务卡或审查请求引用给出）后再运行 `visual-reviewer`。基线冲突时以规范文档为最高判据：目标截图/设计稿只是当轮还原目标，与规范不一致时先按「UI 设计规范」一节走规范修订，不得直接按设计稿判过。截图比较必须使用支持图片输入的模型。
- 截图未回传或回传不齐（缺页面、缺状态、尺寸/主题不符）时，视觉审查只能给出 `INSUFFICIENT_EVIDENCE`，不得标记视觉通过；补齐证据后重跑。缺少明确视觉基线或没有支持图片输入的模型可用时同样只能给出 `INSUFFICIENT_EVIDENCE`。
- 视觉 reviewer 只判断还原度与规范符合度：几何与面板比例、对齐、间距、字体层级、颜色/对比度、组件形态、信息密度、状态表现、溢出/裁切/滚动，以及不同窗口尺寸下的稳定性。它不得把个人审美当作目标，不得扩展产品范围或审查业务逻辑。
- 主会话负责接受或驳回视觉 findings；需要修复时保持视觉 review 前已经形成的 writer 归属。**视觉 review 之后产生的修复增量只允许修改 UI 表现代码**：限 `crates/ui/**`、`crates/app/**` 中纯视图/布局/样式渲染片段，以及直接对应的 UI 测试、视觉 fixture 和视觉资源；禁止修改 `crates/pi-rpc/**`、`crates/pi-data/**`、`crates/pi-render/**`，也禁止改 RPC、进程/会话控制、状态机、数据模型、持久化、协议及其他业务逻辑。
- 若某条视觉 finding 必须依赖业务代码才能解决，立即停止该项视觉修复，将其标记为“非 UI 依赖”，另行进入普通开发与代码 review 流程；禁止借视觉修复顺手改业务代码。视觉修复完成后，先确认相对视觉 review 基线的增量没有越界，再对该 UI 增量完成必要代码复核并重新运行 `visual-reviewer`；最终视觉结论必须为 `PASS` 才能进入用户视觉验收或本轮收口。

## 平台归属

| 范围 | 执行方 |
|---|---|
| R0 工程骨架 | 130 Arch（历史；已迁移） |
| **R1–R18 全部实现、测试、打包** | **Windows** |
| CI | `windows-latest` 阻断（唯一 job） |

**项目已迁移为 Windows solo**：不再维护 Linux 构建、不再跑 Linux CI，`ubuntu-latest` 相关 job 已从 `.github/workflows/ci.yml` 移除。所有 `.sh` 脚本与 Linux 分支说明仅为历史遗留，不再维护更新。从 Linux 交叉编译 Windows 的 GPUI 目标**不可行**（DirectX + COM + MSVC 链接器）。

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
| 颜色/字体 | 一律走 `cx.theme()` token（gpui-component `Theme`），组件内禁止硬编码颜色值和字体名；字体族按平台条件编译，唯一注入点是 `crates/ui/src/theme.rs` 的 theme 初始化，改字体只改那里 |
| 注释 | 中文。写**为什么**，不复述代码在做什么 |
| clippy | `-D warnings`，不许 `#[allow]` 糊过去；确有必要时必须写明理由 |
| PowerShell 脚本 | 改完**必须验证**，不许"照 sh 版翻译完就提交"（R0 因此红过一次 CI）：能跑的直接跑，跑不了的至少过一遍语法解析（`[Parser]::ParseFile`） |
| `$LASTEXITCODE` | PowerShell 只有跑过**外部程序**才写它 —— 调 `.ps1` 且它正常结束时该变量保持旧值、首次调用时是空的。判断成败前先 `$global:LASTEXITCODE = 0`，且被调脚本成功时要显式 `exit 0` |

## UI 设计规范（每轮必守）

- 权威文档：[`docs/UI设计规范.md`](docs/UI设计规范.md)——由 Zed Agent Panel 设计语言调研映射 gpui-component 形成（R9 前置产出），是 UI 视觉的唯一判据；与它冲突时以规范文档为准，与立项文档冲突时以立项文档为准。
- **R9 负责把现有 UI 打磨到符合规范**；R10 起每一轮新增/修改 UI 都必须遵守规范，不得回归旧风格。
- 核心条款（详细条目见规范文档）：
  - 颜色/字体一律走 `cx.theme()` token，禁止硬编码；
  - 层级用背景 + 留白表达，边框只用于 hover / 选中 / 错误态；过程性内容（工具/思考/计划）用弱边框卡片 + header 状态点；
  - 工具输出用左竖线 + 缩进表达从属关系，不嵌套新卡片；
  - 操作三级可见：主操作常驻（primary）、次操作 hover 显隐、低频操作收菜单；
  - 一行信息不超过 3 个片段，次要信息进 tooltip；
  - 状态色只点不铺（小圆点 / 左侧竖条 / 图标色），不铺满卡片边框或背景。
- UI 改动若与规范冲突：先改规范文档（走评审）再改代码，不许绕过规范硬编码。

## 上游源码在哪（只读参考）

| 用途 | 位置 |
|---|---|
| 功能对照基线 pi-web 0.8.9 | 固定 `vendor/upstream/pi-web-0.8.9/`；运行 `.\\scripts\\fetch-pi-web.ps1` 准备，身份钉 `v0.8.9` / `2a6e53710f6409e0cceb3de839a62f8cdf3ca3ca`（`pins/pi-web-0.8.9.manifest` 全量校验） |
| pi 0.84.2 源码（协议、trust 等实现权威参考） | 固定使用 `vendor/upstream/pi-0.84.2/`；运行 `.\\scripts\\fetch-pi-source.ps1` 准备，身份钉 `v0.84.2` / `914cf1472e715297caa30db4b9535d534a9eb718`（`.gpui-pi-source-pin` marker + `pins/pi-0.84.2.manifest` 全量校验）；禁止引用会自动更新的 Pi Agent 安装目录 |
| RPC 协议权威文档 | `vendor/upstream/pi-0.84.2/packages/coding-agent/docs/rpc.md`，或 pi 发布包内 `vendor/pi/docs/rpc.md` |
| 会话文件格式 | `vendor/upstream/pi-0.84.2/packages/coding-agent/docs/session-format.md`，或 pi 发布包内 `vendor/pi/docs/session-format.md` |
| 组件库用法 | `git clone https://github.com/longbridge/gpui-component` 后 checkout 到 `Cargo.lock` 中 `gpui-component` 钉定的 rev（以 lock 文件为准），看 `crates/story/src/stories/`；禁止按 main HEAD 参考 |

## 协作

- worktree + PR，分支 `WinClaude/round-NN`；
- `main` 只接 PR，不直推；
- PR 描述必须贴 validation 的实际回显，不贴不审；
- **合并后必须验证 main 真的包含那些 commit**（`git log main --oneline` 或 `git branch --contains <sha>`）。R0 合并时踩过：最后一个 commit 已经 push 成功（remote-tracking reflog 有记录），但 `gh pr merge` 用的是 GitHub 侧尚未刷新的 PR head，那个 commit 被静默漏掉，而分支随即被 `--delete-branch` 删了。
