# 钉死的 pi 0.84.2 源码参考

> 这是 R0 上游钉版本约定的补充说明。源码只作协议、会话格式与实现行为考古，不作为构建或运行时依赖。

## 唯一约定目录

仓库内统一使用以下固定路径：

```text
vendor/upstream/pi-0.84.2/
```

禁止再引用会随 Pi Agent 桌面应用自动更新而变化的安装目录，例如：

```text
D:/Program Files/Pi Agent/resources/runtime-seed/node_modules/@earendil-works/pi-coding-agent/
```

## 钉死身份

| 项 | 值 |
|---|---|
| 上游仓库 | `https://github.com/earendil-works/pi.git` |
| tag | `v0.84.2` |
| commit | `914cf1472e715297caa30db4b9535d534a9eb718`（轻量 tag，经 GitHub API 独立核实） |
| codeload 归档 SHA256 | `65077457f18f9d3b0bc642870c5c19f41e38378e7f0ba4c3dd0962989e7d0036` |
| 本地目录 | `vendor/upstream/pi-0.84.2/` |
| 源码入口 | `vendor/upstream/pi-0.84.2/packages/coding-agent/` |
| 内容基线 | `pins/pi-0.84.2.manifest`（1373 个文件的 SHA256 + 大小，提交进 git） |

目录名带版本号，后续即使升级 pi，也必须放入新的版本目录，不允许覆盖这个目录。

## 准备与验证

Windows：

```powershell
.\scripts\fetch-pi-source.ps1
```

Linux / macOS：

```bash
./scripts/fetch-pi-source.sh
```

fetch 流程（bash 与 PowerShell 行为一致）：

1. 调 GitHub API 验证远端 `v0.84.2` 指向 `914cf147…`（API 不可达时降级为警告，不阻断）；
2. 下载 codeload tag 归档并校验 SHA256（字节级钉死，tag 被改写或归档被污染都会失败）；
3. 解包到 `vendor/upstream/` 下的临时目录（与目标同卷，最终 `mv` 才是原子 rename，不会跨卷退化成复制）；
4. 写入 `.gpui-pi-source-pin` marker（version / tag / commit / archive_sha256 / source 五行）；
5. 调用 `scripts/check-pi-source-pin.sh` / `.ps1` 对临时目录做**全量**校验，通过才发布。

`scripts/check-pins.*`（validate 第 1 步）每次都会执行 `check-pi-source-pin.*`，校验：

- 目录存在且不含 `.git`（不会被 `git pull` 或分支切换漂移）；
- marker 恰好 5 行且逐行精确匹配（拒绝子串伪造、多余或缺失字段）；
- 除 marker 外全部文件重新计算 SHA256 + 大小，与 `pins/pi-0.84.2.manifest` 基线逐行比对 —— 任意文件增、删、改都会判红并输出差异。

## 注意

- `vendor/` 整体 gitignored；manifest 基线 `pins/` 提交进 git，新 checkout 和 CI 必须先跑 fetch 脚本再跑 validate。
- 反向测试已确认：篡改任意源码文件或 marker 字段都会让 `check-pins` 失败；恢复后通过。
