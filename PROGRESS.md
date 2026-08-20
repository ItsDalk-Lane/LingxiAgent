# PROGRESS — openhanako v0.444.1 → v0.447.4 上游同步

## 审计坐标（已固定，执行期间不得移动）

```
UPSTREAM_BASE_SHA   = cc19cb49b0786d61ed723764e0a83baf87887270  (openhanako v0.444.1)
UPSTREAM_TARGET_SHA = c6d0405294be67cb134c2758f6472748ee73e2be  (openhanako v0.447.4)
LINGXI_BASE_SHA     = 97595264  (chore: sync upstream 0.444.1 and pi SDK 0.84.1, PR #2)
LINGXI_START_SHA    = ca0b417e36a6a1f80947458aaed328a25718e41b  (main HEAD @ 2026-08-20)
工作分支            = feature/upstream-sync-0.447.4
```

ΔU = 18 commits / 133 paths（7850+/738-）；ΔL = 346 paths；overlap = 29 paths。
原始数据：`.sync-audit/delta-U.txt`、`delta-L.txt`、`overlap-paths.txt`、`per-commit-paths.txt`。

## 当前阶段

Phase 0 完成，Phase 1（基线测试）进行中。

## 阶段状态

| Phase | 内容 | 状态 |
|---|---|---|
| 0 | 执行保护文件 + 坐标 + 矩阵 | ✅ |
| 1 | Lingxi 功能基线（全量测试基线） | 🔄 |
| 2 | AGENTS.md 人格协议迁移 | ⬜ |
| 3 | Memory Dream 全链路 | ⬜ |
| 4 | Automation store 恢复 | ⬜ |
| 5 | Markdown 裸 URL | ⬜ |
| 6 | Context Ring 顺序 | ⬜ |
| 7 | Windows seed 清理 | ⬜ |
| 8 | i18n key-level merge | ⬜ |
| 9 | bundled skills 同步 | ⬜ |
| 10 | 依赖判断（上游仅 version bump、无新依赖 → 不动 package.json/lock） | ✅ |
| 11 | release digest → INTENTIONAL_DIVERGENCE | ✅ |
| 12 | derived artifacts 重新生成 | ⬜ |
| 13-15 | typecheck / lint / 定向+全量测试 | ⬜ |
| 16-17 | 构建 + 打包 + 旧数据 migration smoke | ⬜ |
| 收尾 | upstreamVersion→0.447.4 + 最终审计 + 语义提交 | ⬜ |

## 已执行测试

（待记录）

## 尚未执行测试

全部。

## 当前发现

1. 上游 package.json 在 U0..U1 区间**仅有 version 字段变化**，Dream 无新增运行时依赖 → package.json/lock 整体 INTENTIONAL_DIVERGENCE。
2. Lingxi 人格体系仍在 `ishiki` 命名（13 个 TS 文件引用）；模板内含 `lingxi.md` 品牌文件（上游对应 hanako.md）→ 模板迁移=目录改名+保留 lingxi.md。
3. Lingxi 模型解析锚点：`engine.resolveAuxiliaryExecution("memory", { agentId })`（core/engine.ts:1907 + core/auxiliary-model-resolver.ts）→ Dream 与 MemoryTicker 共用此 contract。
4. overlap 29 路径中 C 类最高风险：`AssistantMessage.tsx`（Lingxi 聊天语义架构已重构，automation suggestion 失败必须走 ContentBlock）。
5. `lib/desk/cron-store.ts`、`lib/memory/memory-ticker.ts` 不在 overlap——Lingxi 扩展均在 L0 前并入，上游 patch 基线一致性好，可按 B 类合并。
