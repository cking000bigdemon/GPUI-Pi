# GPUI-Pi

用 [GPUI](https://gpui.rs) + [gpui-component](https://github.com/longbridge/gpui-component) 重写的 **pi 编程智能体原生桌面客户端**。

不是 pi-web 的套壳 —— 没有 Electron、没有 Chromium、没有 Next.js、没有内置 Node，UI 全部原生绘制。
Agent 内核用官方发布的 **pi 独立二进制**（Bun 编译，单文件），通过 `pi --mode rpc` 的 JSONL 协议驱动。

## 与 pi-web-desktop 的关系

| | [pi-web-desktop](https://github.com/cking000bigdemon/pi-agent-desktop) | GPUI-Pi |
|---|---|---|
| 外壳 | Electron | GPUI 原生 |
| UI | `@agegr/pi-web`（Next.js 服务 + Chromium 窗口） | Rust 原生绘制 |
| 内核 | npm 装的 `@earendil-works/pi-coding-agent`（进程内） | 官方独立二进制（子进程 RPC） |
| 内置运行时 | Node + Python + DeepSeek Harness | 无 |
| 安装体积 | 500MB+ | 目标 ≤ 220MB |

两者**共用 `~/.pi/agent/`**（会话、模型凭据、扩展、技能），可并行安装。
GPUI-Pi 不接管扩展/技能的部署，也不内置 DeepSeek Harness —— 那些继续由 pi-web-desktop 负责。

## 上游钉版本

v1 开发期间锁死，不追上游：

| 上游 | 版本 | 角色 |
|---|---|---|
| `agegr/pi-web` | **0.8.9** | 功能对照基线（1:1 复刻目标，不作为运行时依赖） |
| `earendil-works/pi` | **0.84.2** | 运行时内核（独立二进制 `pi-<platform>`） |

## 状态

18 轮拆解开发中，进度见 [`ROUNDS.md`](ROUNDS.md)，设计见 [`docs/立项文档.md`](docs/立项文档.md)。

```bash
./scripts/fetch-pi.sh          # 拉 pi v0.84.2 独立二进制到 vendor/pi/
./scripts/fetch-pi-source.sh   # 拉同版本钉死源码到 vendor/upstream/pi-0.84.2/（含 manifest 全量校验）
./scripts/fetch-pi-web.sh      # 拉功能对照基线到 vendor/upstream/pi-web-0.8.9/（含 manifest 全量校验）
./scripts/validate.sh          # T1 验收（--logic 只跑纯逻辑 crate）
cargo run -p gpui-pi           # 启动原生桌面客户端
```

## License

MIT
