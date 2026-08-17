# 钉死的 pi-web 0.8.9 功能对照基线

> R0 上游钉版本约定的补充说明。pi-web 是功能对照基线（1:1 复刻目标），只读参考、不作为运行时依赖。

## 唯一约定目录

```text
vendor/upstream/pi-web-0.8.9/
```

与 pi 源码一样，禁止把对照基线 clone 到项目根目录或随意的临时位置；也禁止引用
会被桌面应用自动更新覆盖的安装目录。

## 钉死身份

| 项 | 值 |
|---|---|
| 上游仓库 | `https://github.com/agegr/pi-web.git` |
| tag | `v0.8.9`（**注解 tag**，需两级 API 解析：refs → tag 对象 → commit） |
| commit | `2a6e53710f6409e0cceb3de839a62f8cdf3ca3ca` |
| codeload 归档 SHA256 | `9624948a2194e51d6d99208ce74dcd648f4886654d167fefd0afd84588d44883` |
| 本地目录 | `vendor/upstream/pi-web-0.8.9/` |
| 内容基线 | `pins/pi-web-0.8.9.manifest`（380 个文件的 SHA256 + 大小，提交进 git） |

## 准备与验证

Windows：

```powershell
.\scripts\fetch-pi-web.ps1
```

Linux / macOS：

```bash
./scripts/fetch-pi-web.sh
```

流程与 `fetch-pi-source.*` 一致：GitHub API 验证远端 `v0.8.9` → `2a6e5371…`
（注解 tag 两级解析；API 不可达时降级警告）→ codeload 归档 SHA256 校验 →
同卷临时目录解包 → 写入 `.gpui-pi-web-source-pin` marker → 全量 manifest 比对 →
通过后原子发布。

`scripts/check-pins.*`（validate 第 1 步）同时校验 pi 与 pi-web 两个参考目录：
marker 精确 5 字段 + 全量内容与 `pins/pi-web-0.8.9.manifest` 逐行比对，任意文件
增删改都会判红并定位差异。反向测试已确认篡改 `lib/session-reader.ts` 会判红。
