# Round 00 — 工程骨架

> 执行方：130 Arch · 状态：已完成（2026-08-16）

## 目标

建立可编译、可验收、上游版本已钉死的 Rust 工作区骨架，让 R1 起的 Windows 端拿到就能直接开工。

## 前置

- `rustup toolchain install 1.97.1 -c rustfmt -c clippy`
- 网络可达 github.com（要 clone zed 与 gpui-component）

## 交付物

| 文件 | 作用 |
|---|---|
| `Cargo.toml` | workspace，5 个成员 + `[workspace.dependencies]` 集中版本 |
| `rust-toolchain.toml` | 钉 1.97.1 |
| `Cargo.lock` | **真正的上游钉死点**（gpui 不带 rev，见下方设计说明） |
| `crates/pi-rpc/` | 纯逻辑：版本常量、平台二进制名、发布包目标 |
| `crates/pi-data/` | 纯逻辑：`~/.pi/agent` 目录解析 |
| `crates/pi-render/` | 纯逻辑：渲染中间模型骨架 |
| `crates/ui/` | 依赖 gpui + gpui-component，验证依赖链可编译 |
| `crates/app/` | 可执行文件，打印环境自检（**不开窗口**） |
| `scripts/fetch-pi.{sh,ps1}` | 按平台拉 pi v0.84.2 + SHA256 校验 + 版本自检 |
| `scripts/check-pins.{sh,ps1}` | 校验 `Cargo.lock` 里的上游 sha |
| `scripts/validate.{sh,ps1}` | T1 五步验收，支持 `--logic` 快速模式 |
| `.github/workflows/ci.yml` | windows 阻断 + linux 纯逻辑非阻断 |
| `CLAUDE.md` / `AGENTS.md` | 每轮必守的操作约定 |
| `ROUNDS.md` | 轮次进度表 |
| `rounds/{TEMPLATE,BACKLOG,round-00,round-01}.md` | 轮次机制 |
| `.gitattributes` | 全仓 LF |

## 设计说明：为什么 gpui 依赖不写 rev

gpui-component 自己对 zed 的依赖是**不带 rev 的 git 依赖**。如果我们这边写 `rev = "..."`，cargo 会认为同一个 git URL 出现了两个不同的 reference 而拒绝解析。

所以：**依赖声明与 gpui-component 保持一致（不带 rev），真正的钉死落在提交进仓库的 `Cargo.lock`**，由 `scripts/check-pins.*` 在每次 validate 时校验 sha。谁跑了 `cargo update` 谁红。

## 验收

| 级别 | 检查 | 命令 / 期望 |
|---|---|---|
| T1 | 钉版本 | `./scripts/check-pins.sh` 两条 OK |
| T1 | 格式 / lint / 测试 / 构建 | `./scripts/validate.sh --logic` → `VALIDATE OK` |
| T2 | pi 二进制供给 | `./scripts/fetch-pi.sh` → `OK vendor/pi/pi (0.84.2)` |
| T2 | 版本同源 | `cargo test -p pi-rpc` 的 `pinned_version_matches_fetch_scripts` 通过 |
| T3 | CI | PR 上 windows job 绿 |

> 原计划以 `--logic` 为硬验收、全量编译交给 CI 的 windows job。实测 130 上**全量也过了**（见下），所以两档都算数。

## 禁止

- 不开窗口、不写任何 UI 组件（那是 R1/R4）；
- 不实现 RPC 协议、不解析会话文件（R2/R3）；
- 不引入 WebView、不加 web 技术栈依赖；
- 不把 `vendor/` 提交进仓库。

## 失败处理

连续 2 次 validation 不过 → 写 `rounds/BLOCKED-00.md`，停下呼人。

## 本轮实测

### 结果

| 检查 | 结果 |
|---|---|
| `./scripts/check-pins.sh` | 4 条 OK（含新加的"无杂散 sha"两条） |
| `./scripts/validate.sh --logic` | `VALIDATE OK`，7 个单测全过 |
| `./scripts/validate.sh`（**全量，含 GPUI**） | `VALIDATE OK` —— release 编译 853 个包耗时 **2m28s** |
| `./scripts/fetch-pi.sh` | `OK vendor/pi/pi (0.84.2)`，SHA256 校验通过 |
| `cargo run -p gpui-pi` | 环境自检输出正常 |

### 踩到的坑：`cargo update --precise` 会把同一 git 源劈成两半

按顺序跑 `cargo update gpui --precise <zed-sha>` 再跑 `cargo update gpui-component --precise <sha>` 之后，`Cargo.lock` 变成**混合状态**：

```
f4199ae0  17 个包（gpui, collections, refineable, sum_tree …）  ← 被第二条命令带到了 HEAD
cc053a4a   6 个包（gpui_platform, gpui_linux, gpui_windows …）  ← 留在钉的 sha
000114aa   4 个包（gpui-component …）
```

cargo 不报错，但这是一份半新半旧的锁 —— gpui 与 gpui_platform 来自两个不同 commit。

**修法**：把 zed 源下的全部 23 个包名一次性传给同一条 `cargo update --precise`，让它们整体归位。最终 22 个包全在 `cc053a4a`（一个包在新解析中消失了）。

**防复发**：`check-pins.*` 加了 `check_no_stray` —— 不只检查"钉的 sha 在场"，还要检查"这两个 git URL 下不存在第二个 sha"。这一条是本轮真正值钱的产出。

### 意外收获：Linux 能全量编译 GPUI

原以为 130 上编不了（要 Wayland/X11/Vulkan 一堆 `-dev` 库），实测**开箱即过**，Arch 的 mesa/wayland/libxkbcommon 已经够了，`gpui_linux` / `gpui_wgpu` / `gpui_platform` 全部编译通过。

意义：**130 可以给 UI 层做"能不能编过"的快速兜底**，虽然按立项文档 § 八 它不承担 UI 开发。CI 的 linux job 仍保持 `--logic`（GitHub runner 上装那套 apt 依赖不划算）。

### 不能当真的数字

`target/release/gpui-pi` 只有 **1.9MB** —— 因为 R0 的 `main()` 根本没调用 GPUI，链接器把整个渲染栈裁掉了。真实体积要等 R1 开出窗口后再测，立项文档里 50–90MB 的估计**尚未验证**。

### 留给 R1 的三件事

1. `rustup toolchain install 1.97.1-x86_64-pc-windows-msvc` + Build Tools + 长路径开关；
2. `.\scripts\fetch-pi.ps1` 与 `.\scripts\check-pins.ps1` **在 130 上无法执行验证**，两个 PowerShell 脚本是照着 sh 版翻的，R1 第一件事就是跑通它们；
3. spike 结束后把 spike 代码的去留记进 `BACKLOG.md`。
