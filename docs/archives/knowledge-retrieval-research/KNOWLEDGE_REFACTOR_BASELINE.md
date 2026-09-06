# 知识重构固定基线

> 归档于 2026-09-06：以下源码路径、命令和状态按原任务的仓库根目录与日期理解；档案目录不是命令运行目录。参见[档案索引](../README.md)。

- 基线 SHA：`3eab85891a1747c64064252804f70c0a3773f021`
- 分支：`feat/knowledge-retrieval-research-p0-p3`
- Node：`v24.16.0`
- npm：`11.13.0`
- 操作系统：macOS `26.6.2`（25G83），Darwin 25.6.0，arm64
- typecheck：PASS，`npm run typecheck`，exit 0，三套类型检查完成
- knowledge 定向测试：PASS，5 文件 / 110 测试，0 失败 / 0 跳过，8.06 秒，exit 0
- 全量测试：PASS，1273 文件通过 / 1 文件跳过，12929 测试通过 / 7 跳过 / 0 失败，89.41 秒，exit 0
- 现有快速模式典型分段耗时：见下表（本地 fixture，不代表远程供应商耗时）

## 环境与步骤记录

- 初始 HEAD 与固定基线一致；只有用户任务书未跟踪，无既有代码修改。
- 已完成 git fetch origin、检出固定基线并创建任务书指定分支。
- npm ci 已完成（exit 0），日志：`/tmp/lingxi-knowledge-p0-npm-ci.log`。

## 基线命令与证据

```bash
npm run typecheck
npx vitest run tests/knowledge-context-injector.test.ts tests/knowledge-rerank-fusion.test.ts tests/knowledge-retrieval-golden.test.ts tests/knowledge-query-config-alignment.test.ts tests/knowledge-read-tool.test.ts
npm test
```

- 类型日志：`/tmp/lingxi-knowledge-p0-typecheck.log`。
- 定向日志：`/tmp/lingxi-knowledge-p0-targeted.log`。
- 全量日志：`/tmp/lingxi-knowledge-p0-full.log`（exit 0）。

## 旧快速路径分段计时

复用固定基线 golden fixture 的 5 份中英文文档、2 个笔记本、6 个问题，每题 10 次，共 60 次；实际经过 KnowledgeManager 摄入、SQLite FTS、portable vector、融合和旧 injector。嵌入使用 fixture 内本地确定性函数，没有真实远程供应商调用，未配置 rerank。测量期间全量测试也在运行。

测量脚本：`/tmp/lingxi-knowledge-p0-timings.mts`；原始数据：`/tmp/lingxi-knowledge-p0-timings.json`。命令：`node --experimental-transform-types /tmp/lingxi-knowledge-p0-timings.mts`，exit 0。

| 阶段 | P50（ms） | P95（ms） | 最大（ms） |
| --- | ---: | ---: | ---: |
| assembleMs | 0.0 | 1 | 1 |
| embedMs | 1.0 | 1 | 7 |
| ftsMs | 0.0 | 1 | 1 |
| fuseMs | 0.0 | 0 | 1 |
| plannerMs | 0.0 | 0 | 0 |
| totalMs | 2.0 | 2 | 11 |
| vectorMs | 0.0 | 1 | 6 |

这些毫秒计时采用旧代码的 Date.now 分辨率。未测量真实远程供应商延迟，不能用本表证明现有快速路径满足纯本地零调用或大语料性能目标。

## 基线结论

- 任务书指定基线三项验证全部通过；现有 1 文件 / 7 测试跳过为固定基线原状，本任务未新增跳过。
- 旧快速路径允许查询嵌入等待 15000ms、条件性重排等待 5000ms；这是源码期限，不是本次实测延迟。
- 本阶段未修改生产代码、测试、依赖清单或生成物；只新增本基线文档与任务进度文档。
- 发现后续阶段门禁依赖：现有审计封印比较已验证提交到 HEAD 的所有文件，而任务书新文档不在既有审计名单；首个任务提交后该门禁预计会拒绝。P0 阶段收口时必须保留真实失败状态，不得放宽名单或假造封印。
