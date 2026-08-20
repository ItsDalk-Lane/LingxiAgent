# PROGRESS — openhanako v0.444.1 → v0.447.4 上游同步

## 审计坐标（已固定，执行期间从未移动）

```
UPSTREAM_BASE_SHA   = cc19cb49b0786d61ed723764e0a83baf87887270  (openhanako v0.444.1)
UPSTREAM_TARGET_SHA = c6d0405294be67cb134c2758f6472748ee73e2be  (openhanako v0.447.4)
LINGXI_BASE_SHA     = 97595264  (chore: sync upstream 0.444.1 and pi SDK 0.84.1, PR #2)
LINGXI_START_SHA    = ca0b417e36a6a1f80947458aaed328a25718e41b  (main HEAD @ 2026-08-20)
LINGXI_FINAL_SHA    = （见 git log，收尾提交链末端）
工作分支            = feature/upstream-sync-0.447.4
```

ΔU = 18 commits / 133 paths（7850+/738-）；ΔL = 346 paths；overlap = 29 paths。
原始数据：`.sync-audit/delta-U.txt`、`delta-L.txt`、`overlap-paths.txt`、`per-commit-paths.txt`。

## 当前阶段

**全部完成（含收尾）。** 14 个语义提交落在 feature/upstream-sync-0.447.4。

## 阶段状态（终态）

| Phase | 内容 | 状态 | 证据 |
|---|---|---|---|
| 0 | 执行保护文件 + 坐标 + 矩阵 | ✅ | 4f5b2d00 |
| 1 | Lingxi 功能基线（1125 文件 11436 用例全绿 / 7 skipped） | ✅ | Phase 1 记录 |
| 2 | AGENTS.md 人格协议迁移 | ✅ | f781f10f, cb4647d1, f32c3f64 |
| 3 | Memory Dream 全链路 | ✅ | e715b8e4, 9e2fa339, 8f249913 |
| 4 | Automation store 恢复 | ✅ | 27d14477, ba9cb461 |
| 5 | Markdown 裸 URL | ✅ | fb032eea |
| 6 | Context Ring 顺序 | ✅ | 63bc92b7 |
| 7 | Windows seed 清理 | ✅ | 18727d24 |
| 8 | i18n key-level merge（5 locale） | ✅ | 8f249913（同提交含 locale） |
| 9 | bundled skills 同步 | ✅ | f32c3f64 |
| 10 | 依赖判断（上游仅 version bump、无新依赖） | ✅ | package.json/lock INTENTIONAL_DIVERGENCE |
| 11 | release digest → INTENTIONAL_DIVERGENCE | ✅ | 未拷贝任何上游 digest/receipt |
| 12 | derived artifacts 重新生成 | ✅ | abbfb593 |
| 13-15 | typecheck / lint / 定向+全量测试 | ✅ | 见下「已执行测试」 |
| 16-17 | 构建 + 打包 + 旧数据 migration smoke | ✅ | 见下「已执行测试」 |
| 收尾 | upstreamVersion→0.447.4 + 最终审计 + 语义提交 | ✅ | 收尾提交 |

## 已执行测试（终态全绿）

1. `npm run typecheck` — tsc×3（root + node + test）全绿。
2. `npm run lint` — 0 errors / 8113 warnings（main 基线 8037，新增 76 条均为既有风格类 warning，无 error）。
3. `npm run lint:boundary` — open 边界检查通过（含 createMemoryDreamRoute 挂载清单更新）。
4. 定向测试 — persona migration / workspace injection / Dream 核心 / Dream memory slot 契约 / cron-store 恢复 / Automation UI / markdown URL / context-ring / windows installer contract / locale 完整性 / export manifest / persistence fingerprint 全部通过。
5. `npm test` 全量 — **1136 文件通过 | 1 skipped；11532 用例通过 | 7 skipped；0 failed**（69.9s）。
   - 过程中曾出现「12 failed files / 0 failed tests」假象：`.sync-audit/3way/` 三方合并草稿文件被 vitest 测试发现误捕获，删除草稿目录后消失，非真实失败。
6. `npm run build:server` + `build:server:open` + renderer/preload/main/splash/theme 构建全绿（构建签名用一次性 throwaway keypair，未写入仓库）。
7. `npm run pack`（SKIP_NOTARIZE=true，electron-builder --dir）exit 0 → `dist/mac-arm64/Lingxi.app` 产出正常。
8. 打包产物抽检：seed tarball 含 `agents-templates`；全产物 grep 无 `ishiki` 残留；`dist-server/index.js` 含 `createMemoryDreamRunner` 与全部 `dream_*` 错误码。
9. 旧数据 migration smoke（`.sync-audit/migration-smoke.mjs`，16 项断言）：ishiki.md→AGENTS.md 改名、memory/facts/daily/cron/sessions/providers/user 全量完好、双文件并存→`.pre-agents-rename.bak` 保留、首跑清洁、失败不删文件不阻塞启动 — **16/16 通过**。
10. Windows installer contract（tests/windows-installer-contract.test.ts，19 用例）通过——含 `lingxiRemoveOwnedInstallTrees` 宏引用修正。真实 Windows 安装器执行受宿主平台限制，仓库门禁即此 contract 测试。

## 尚未执行测试

无（收尾时 upstreamVersion bump 为纯元数据变更，不影响代码路径；matrix/audit 文档为文本）。

## 最终发现与处置（全量）

1. 上游 package.json 在 U0..U1 区间**仅有 version 字段变化**，Dream 无新增运行时依赖 → package.json/lock 整体 INTENTIONAL_DIVERGENCE；Pi SDK 保持 0.84.1 未降级。
2. Lingxi 人格体系原 `ishiki` 命名整体迁往 `AGENTS.md` 协议；模板 `lingxi.md` 与上游 `hanako.md` 字节一致（纯品牌改名）→ 模板迁移=目录改名+保留品牌文件。
3. Dream 绑定 Lingxi 辅助模型槽：`agent.getResolvedMemoryModel` 闭包 → `memory-ticker` → `dream runner` 注入链，仅解析 `("memory", { agentId })`，以 tests/memory-dream-memory-slot.test.ts 锁定，未复活 utility 架构。
4. overlap 29 路径中 C 类最高风险 `AssistantMessage.tsx` 以干净三方合并落地：automation suggestion 失败走 ContentBlock 架构，Lingxi 聊天语义管线零回归。
5. `lib/desk/cron-store.ts` 与 `memory-ticker.ts` 上游 patch 基线一致性好，直接/B 类合并；Lingxi 扩展（configRevision/storeRevision）保全。
6. 派生 artifact 重新生成：CLI closure、persistence schema fingerprint（compatible 增补，DATA_EPOCH 不变）、persistent stores scan、export-manifest 704→712。
