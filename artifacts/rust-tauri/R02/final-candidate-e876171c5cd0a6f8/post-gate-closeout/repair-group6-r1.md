# R02 Final Repair — Group 6 R1: API 兼容矩阵重新生成

- 子代理: `R02-REPAIR-GROUP-6-R1`
- 仓库: `/Users/study_superior/Desktop/Code/LingxiAgent`, 分支 `codex/rust-tauri-migration`, HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`
- 任务: 根因组 6 — R17 客户端接线新增 API 后，已提交的 `API_COMPAT_MATRIX.json` 未重新生成，导致 `check-contracts` 第 2 步 (extract --check) 漂移。
- 结论: **已修复，无删除项，check-contracts exit 0**。

## 1. 生成器事实（先读后动）

- 检查脚本: `scripts/rust-tauri/r01-t02-check-generated.sh` — 两步门禁：[1/2] `lingxi-protocol-gen --check`（contracts/generated 56 文件）；[2/2] `node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --check`。
- 生成器: `scripts/rust-tauri/r01-t02-extract-api-surface.mjs`。
  - 输入面（extract 步）: `server/index.ts`、`server/composition/open-root.ts`、`server/composition/full-root.ts`、`server/routes/*.ts`（HTTP/WS 路由）、`desktop/preload.cjs`（window.hana 成员 + IPC 通道）、`desktop/src/react/types.ts`（PlatformApi 成员）。
  - 产物: `docs/rust-tauri/R01/API_COMPAT_MATRIX.json`（唯一写出文件；types.ts 等均为只读输入，生成器不写它们）。
  - `--check` 为全文件逐字节 diff，无字段豁免；`contentSha` 是内容派生戳，不嵌 git HEAD。

## 2. 根因确认

HEAD 提交 `cdd213078`（"R02 修复轮 R17–R21 收口"）改动：
- `desktop/preload.cjs` (+1 行): 在 `getServerToken` 与 `runEditCommand` 之间新增桥成员
  `getServerConnectionInfo: () => ipcRenderer.invoke('get-server-connection-info'),`
- `desktop/src/react/types.ts` (+2/-1): PlatformApi 新增可选成员
  `getServerConnectionInfo?(): Promise<{ port: number | null; token: string | null; serverNodeKind: string | null; serverNodeTransport: string }>;`
  并将 `onServerRestarted?` 回调 payload 增加可选字段 `serverNodeKind?` / `serverNodeTransport?`（**加宽，非收窄**；矩阵只记成员名，不影响条目）。

但该提交未重新生成矩阵 → gate 第 2 步漂移（门禁日志 `/tmp/r02-final/gate/04-check-contracts/stderr.log`：磁盘 `contentSha 2f55e3dd…` vs 生成 `f0e44e6e…`，且 preload 段条目因中间插入而"错位"）。门禁 diff 里仅 `desktop/preload.cjs` 与 `desktop/src/react/types.ts` 两个 sourceDigest 不一致，所有 `server/*` 摘要一致 → 其余输入面无变化。

## 3. 执行记录（全部实际运行）

| # | UTC | 命令 | exit |
|---|-----|------|------|
| 1 | 2026-09-28T15:32:09Z | 快照 `cp docs/rust-tauri/R01/API_COMPAT_MATRIX.json /tmp/r02-final/matrix-before-group6.json`（624 条目, 7215 行） | 0 |
| 2 | 2026-09-28T15:32:13Z | `node scripts/rust-tauri/r01-t02-extract-api-surface.mjs` → `wrote docs/rust-tauri/R01/API_COMPAT_MATRIX.json: 626 entries (423 http, 2 ws, 96 preload, 105 platformApi)` | 0 |
| 3 | ~2026-09-28T15:32:2xZ | `node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --check` → `OK: API_COMPAT_MATRIX.json matches regeneration (626 entries)` | 0 |
| 4 | 2026-09-28T15:33:2xZ 前 | `env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u all_proxy -u ALL_PROXY PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_DIR=/tmp/rust-target-r02-final cargo run --manifest-path rust/Cargo.toml -p xtask --quiet -- check-contracts` → `[1/2] OK: 56 generated files match regeneration`; `[2/2] OK: API_COMPAT_MATRIX.json matches regeneration (626 entries)`; `OK: all generated artifacts are drift-free` | **0** |

## 4. Diff 审查（逐条，git diff -- docs/rust-tauri/R01/API_COMPAT_MATRIX.json）

统计: 1 file changed, 27 insertions(+), 8 deletions(-)。用结构化集合比对（按 id，前/后全量 624→626）复核：

- **新增（2 条，全部为 R17 接线，id 精确列出）**:
  1. `preload:getServerConnectionInfo` — kind `preload_bridge`, category `native_host`, disposition `native_host_mapping`, 插入位置与源码序一致（`preload:getServerToken` 之后、`preload:runEditCommand` 之前）。
  2. `platform_api:getServerConnectionInfo` — kind `platform_api_type`, category `native_host`, disposition `native_host_mapping`, 插入在 `platform_api:getServerToken` 之后。
- **删除: 0 条**（集合比对 removed ids 为空；git diff 为纯插入）。
- **既有条目内容变化: 0 条**（同 id 条目前后 JSON 逐字节一致；423 http + 2 ws 全部原样，`business_compat` 427 不变）。
- 其余变化仅为派生字段: `contentSha` `2f55e3dd…`→`f0e44e6e…`；`sourceDigests` 中 `desktop/preload.cjs` `19fb7dd9…`→`a39d002e…`、`desktop/src/react/types.ts` `7c290e67…`→`27262feb…`（与门禁日志 "gen" 侧完全一致）；summary 计数 `total 624→626`, `preload_bridge 95→96`, `platform_api_type 104→105`, `native_host 197→199`, `native_host_mapping 197→199`。
- types.ts 未被生成器写出（生成器只写矩阵），无需额外审查产物；其源码改动本身为可选字段加宽，无语义收窄。

## 5. 备注观察（不构成漂移，未做任何手改）

- 新 IPC 通道 `get-server-connection-info` 未进入矩阵 `preloadIpcChannels` 清单（106 条前后不变）：生成器通道正则 `ipcRenderer\.(invoke|on|send)\(\s*"…"` 只匹配双引号，而新行用的是单引号 `ipcRenderer.invoke('get-server-connection-info')`。这是生成器既有行为，磁盘与再生成两侧一致，不产生漂移；后续若要收口，应改生成器正则并重新生成（超出本组授权范围，留待总控决断）。

## 6. 变更文件清单（本子代理）

- `docs/rust-tauri/R01/API_COMPAT_MATRIX.json`（仅此一个，由生成器写出，未手改）。
- 未触碰: `scripts/rust-tauri/r01-t02-*` 两个脚本、三个 `r02_*` 脚本、`rust/crates/**`、`desktop/**` 等其他文件（工作区中其余 modified 文件为并行代理所有）。
- 未 commit / 未 push（按要求）。
