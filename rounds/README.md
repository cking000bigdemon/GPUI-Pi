# rounds 目录约定

`rounds/` 保存轮次计划与轮次级管理产出，不保存源码、构建产物或大体积原始日志。

## 目录结构

```text
rounds/
├── README.md
├── TEMPLATE.md
├── BACKLOG.md
├── round-00/
│   └── round-00.md
├── round-01/
│   └── round-01.md
└── round-NN/
    ├── round-NN.md
    ├── BLOCKED.md       # 仅在触发阻塞规则时创建
    └── <其他轮次级文档>.md
```

## 放置规则

- `rounds/` 根目录只放跨轮次文件：本说明、任务卡模板和全局 backlog。
- 每轮先从 `TEMPLATE.md` 创建 `rounds/round-NN/round-NN.md`，再开始实现。
- 该轮的实测记录默认回填任务卡；若内容过长，可拆成同目录下的独立 Markdown，并从任务卡链接。
- 阻塞报告固定为 `rounds/round-NN/BLOCKED.md`，不再使用根目录下的 `BLOCKED-NN.md`。
- 源码、脚本、测试 fixture 仍放 `crates/`、`scripts/`、`tests/` 等标准位置，不复制到轮次目录。
- 本地截图、原始 validation 日志等大文件放 gitignored 的 `.pi/`；需要留档时，在任务卡记录摘要、关键数字和可复现命令。

## 示例

```bash
mkdir -p rounds/round-02
cp rounds/TEMPLATE.md rounds/round-02/round-02.md
```
