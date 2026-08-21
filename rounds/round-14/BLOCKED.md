# Round 14 — BLOCKED

> 状态：整改完成，等待 PR #26 GitHub CI 复验

## 阻塞摘要

PR [#26](https://github.com/cking000bigdemon/GPUI-Pi/pull/26) 已创建，R14 已集成最新 `origin/main@ed554ac`，PR 当前可合并（`mergeable=MERGEABLE`），但唯一 Windows 阻断 CI 连续两次在同一个 **R16 / main 已有测试**失败：

```text
model_service::tests::cli_stdout_is_drained_while_child_runs_and_is_bounded
crates/app/src/model_service.rs:1525
expected: Err(ModelServiceError::CliOutputTooLarge)
```

GitHub Actions：

- Run: https://github.com/cking000bigdemon/GPUI-Pi/actions/runs/32446067832
- Attempt 1 job: `96665747149` — failure
- Attempt 2 job: `96666874461` — failure
- 两次均为同一断言；其余 R14 / R13 / R15 / R16 tests 在 CI 中通过到该点。

## 已确认事实

1. `crates/app/src/model_service.rs` 与 `origin/main` **字节完全一致**：

```text
origin/main blob hash: ed4ba02113619e7ff672a3255fed34401c805a28
R14 worktree hash:      ed4ba02113619e7ff672a3255fed34401c805a28
```

2. 该文件不是 R14 实现或冲突解决产生的修改；它由已经合入 main 的 R16 引入。
3. 本地针对性连续运行该测试 12 次，全部通过；集成后的完整 `scripts/validate.ps1` 也输出 `VALIDATE OK`。
4. R14 官方 pi 0.84.2 Extension UI zero-token fixture 通过：`1 passed / 0 failed`。
5. PR head：`98c127e5ebdf577615fba2f4db15177632fa0538`；工作区干净；R14 已包含最新 main。

## 停止与授权记录

项目红线要求：

- 不跨轮次顺手修改前序轮次问题；发现后记录并呼人。
- 同一验收项连续 2 次仍不过，必须写 `BLOCKED.md` 停下，禁止继续重跑或放宽标准。

因此在两次相同 CI 失败后先停止。随后用户明确授权 **R14 承接该跨轮次 CI flake 整改**，允许修改 `crates/app/src/model_service.rs` 的对应测试，但不得改变生产行为、输出上限、timeout/error mapping，亦不得把 `CliTimeout` 接受为成功。

## 授权后的整改

根因是原 fixture 通过 CMD 执行 20,000 次 `<nul set /p` 逐小块生成输出；共享 `windows-latest` runner 上可能在超过 256 KiB 前先撞到 5 秒 timeout，使严格的 `CliOutputTooLarge` 断言失败。

整改仅替换测试 fixture：

- 测试先用 `fs::write` 创建固定字节 payload；
- `.cmd` 通过一次 `type "%~dp0..."` 批量输出，`%~dp0` 安全解析脚本自身目录并覆盖含空格路径；
- 边界样本固定为恰好 `MAX_CLI_OUTPUT_BYTES`，必须成功、完整且字节不变；
- 超限样本固定为 `MAX_CLI_OUTPUT_BYTES + 1`，仍严格只接受 `CliOutputTooLarge`；
- 失败信息打印实际 error/status/bytes，便于 CI 诊断。

未修改生产函数、`MAX_CLI_OUTPUT_BYTES`、5 秒测试 timeout 或错误映射。

## 整改验证

- focused test 连续运行 20 次：`20/20 passed`，单次约 0.41–0.50 秒；
- `scripts/validate.ps1`：`VALIDATE OK`（app `120 passed / 0 failed / 1 ignored`，其余 workspace suites 全绿，release build 通过）；
- 官方 pi R14 zero-token fixture：`1 passed / 0 failed / 11 filtered out`；
- GitHub CI：等待父会话提交、push 后复验。

历史两次 CI 失败证据保留在上文；当前不再等待授权，状态为“整改完成，等待 CI”。
