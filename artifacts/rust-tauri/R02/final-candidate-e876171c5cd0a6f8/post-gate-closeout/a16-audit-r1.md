# R02-A16 语义审计报告（A16-AUDITOR-R1）

- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`
- 分支：`codex/rust-tauri-migration`，HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`，工作区干净（本审计只读，未修改任何仓库文件）。
- 日期：2026-09-28
- 审计对象：R02-A16「不影响旧入口」的**正确语义**（现行总控指令）下的三条启动链真值 + 当前 A16 gate（`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`，1964 行）的逐项判定。

**正确语义（总控指令）**：A16 = 当前 Electron/Node 生产默认入口没有被 R02 切换 + 现有客户端受影响回归没有新增失败。A16 **不是** desktop/tests/package.json 相对 R01 零修改，也**不是**源码不得出现 Rust service 引用。opt-in Rust preview、显式 runtime 选择、接线准备可以存在。

---

## 1. 三条启动链的真实代码证据

### 1.1 DEFAULT STARTUP PATH —— 结论：**默认 = Node/Electron 现役路径**

判定分支的唯一开关是环境变量 `LINGXI_DESKTOP_SERVER_RUNTIME`，**默认 `node`**，且该变量在整个仓库只出现于一处源码（无 settings、无构建旗标、无 package.json 注入）：

- `desktop/src/shared/rust-local-service.cjs:186-192`
  ```js
  function rustDesktopEnabled(env = process.env) {
    const runtime = env.LINGXI_DESKTOP_SERVER_RUNTIME || 'node';
    if (runtime !== 'node' && runtime !== 'rust') {
      throw new Error(`invalid LINGXI_DESKTOP_SERVER_RUNTIME: ${runtime}`);
    }
    return runtime === 'rust';
  }
  ```
  全仓库 grep `LINGXI_DESKTOP_SERVER_RUNTIME`（desktop/cli/scripts，排除 node_modules/dist 产物）只命中 `rust-local-service.cjs:187,189`。package.json scripts（`start`/`start:dev`/`start:vite`/`cli`/`server`，package.json:30-44）与 `scripts/launch.js`、`desktop/bootstrap.cjs` 均不设置该变量。

启动链：
1. `package.json` `"main": "desktop/bootstrap.cjs"`（gate E1 也断言此值，当前仍成立）。
2. `desktop/bootstrap.cjs:187-195` 加载 `main.cjs`（打包版 `main.bundle.cjs`）。
3. `desktop/main.cjs:110-114` 从 `./src/shared/rust-local-service.cjs` 引入 `rustDesktopEnabled` 等。
4. 所有 server 启动调用点汇入同一个分支 `desktop/main.cjs:1437-1441`：
   ```js
   async function startServer() {
     if (rustDesktopEnabled()) { await startRustServer(); return; }
     serverNodeKind = null; serverNodeTransport = 'http';
     // —— 以下为现役 Node server 逻辑（server-info.json 复用/清理/spawn）
   ```
   `startServer()` 调用点：main 启动流程 `main.cjs:6579`、崩溃自动重启 `main.cjs:2246`、train-update 重启 `main.cjs:5459`。三处共用同一 gate。
5. Node 分支读写 `lingxiHome/server-info.json`（main.cjs:1444 起），`serverNodeKind` 保持 `null`（main.cjs:694 声明、1442 重置）。

渲染层不做任何运行时选择，只被动消费 main 发布的连接信息：IPC `get-server-connection-info` 返回 `{port, token, serverNodeKind, serverNodeTransport}`（main.cjs:5567-5572）；`serverNodeKind` 只在 Rust 分支被设为 `'lingxi-service'`（`publishRustDesktopConnection`，main.cjs:1319-1324）。渲染层 `createLocalServerConnection` 仅当字段存在时校验其为 `'lingxi-service'`（`desktop/src/react/services/server-connection.ts:211-212`），`app-init.ts:227-233` / `SettingsContent.tsx:222` 等 Rust 分支只是**适配降级提示**（`status.rustCoreUnavailable`），不发起选择。

**结论：PRODUCTION DEFAULT = Node/Electron。** 不设 `LINGXI_DESKTOP_SERVER_RUNTIME` 时（普通 `npm run start:vite` 与安装包启动即是），Electron 走现役 Node server 路径；Rust 分支完全不可达。

### 1.2 EXPLICIT RUST PREVIEW PATH —— 显式 opt-in，不选择即不启动、不双写

**桌面**：唯一入口 = 设置 `LINGXI_DESKTOP_SERVER_RUNTIME=rust`（rust-local-service.cjs:186-192）。此时 `startRustServer()`（main.cjs:1326-1435）：
- 拒绝 `LINGXI_ALLOW_DATA_DOWNGRADE=1`（main.cjs:1327-1329）；
- **同宅互斥**：`{home}/server-info.json` 存在即拒绝启动（`RUST_DESKTOP_NODE_SERVER_INFO_PRESENT`，main.cjs:1330-1334）；
- 二进制解析：打包版固定 `resources/rust-service/`（manifest 校验，rust-local-service.cjs:194-215）；开发版 `LINGXI_SERVICE_BIN` 或 `rust/target/debug/lingxi-service`；
- READY 行 `LINGXI_SERVICE_READY addr=… home=…`（main.cjs:1390）+ 实例记录/令牌复核 + 身份验证（`verifyRustDesktopIdentity`，main.cjs:1293-1317）后才 `publishRustDesktopConnection`。

**CLI**：`cli/args.ts:18` 默认 `runtime: "node"`；`--runtime rust` 显式选择（args.ts:63-69，help 文案 args.ts:158 明示 `default: node`）。`cli/entry.ts`：
- `serve --runtime rust` → `spawnRustServerForeground`（entry.ts:35-48）；否则 Node `spawnServerForeground`（entry.ts:49-55）。
- `status/sessions/chat/continue --runtime rust` → Rust 客户端分支（entry.ts:79-145）；否则回落 `resolveConnection` Node 路径（entry.ts:147-158）。
- `cli/server-runner.ts:112-163` `resolveRustServerSpawnSpec`：必须显式 `--home`/`LINGXI_HOME`/`--config`/`--test-mode`（否则报错，server-runner.ts:146-148）；`spawnRustServerForeground` 在数据宅已有 Node server 时拒绝启动（server-runner.ts:182-191，经 token 探测确认后 `A Node server already owns this data home`）。
- 默认连接源：`cli/local-server.ts:21,72` `resolveConnection` 读 `server-info.json`（Node 记录）。

**数据根隔离（不双写）**：
- Rust 全部运行时记录收敛在 `{home}/lingxi-service/`（`instance.lock` 单写锁、`instance.json`、`local-token.json`、`tmp/`，见 `rust/crates/lingxi-service/src/paths.rs:10-40`，0700 权限）；Node 用 `{home}/server-info.json`。两套记录文件互不重叠。
- 双向互斥：桌面 Rust 起动前拒绝存在 `server-info.json`（main.cjs:1330-1334）；CLI Rust serve 起动前探测拒绝 Node 占宅（server-runner.ts:182-191）。Rust 侧 `auth.rs` 对 `server-info.json` 只有注释性语义对照（auth.rs:12），不写该文件。
- 未选择 Rust 时：不 spawn Rust 进程、不创建 `lingxi-service/` 运行时目录、不触碰 Rust 令牌。

### 1.3 REMOTE RUST PATH —— 存在，且为用户显式发起的手动连接

- **渲染层手动连接**：`desktop/src/react/services/server-connection.ts:333-391` `connectDeviceServerConnection({baseUrl, credential})`：先试 Node 协议（`/api/web-auth/login`）；当 Node 路由 401/403/404 且 `/lingxi/v1/health` 确认 `serverKind === 'lingxi-service'` 时切换 Rust 协议（`/lingxi/v1/web-auth/login` + `/lingxi/v1/server/identity`，并校验身份 `serverNodeKind === 'lingxi-service'`，server-connection.ts:352-384）。连接类型 `custom_remote`（trustState `tunnel`，server-connection.ts:322）。
- **CLI 显式远端**：`--url <origin> --token`（任意 http(s) origin，不限于 loopback）+ `--runtime rust` → `explicitRustConnection`（cli/rust-service.ts:263-277，entry.ts:80-82）。本地令牌路径 `readRustLocalService` 则强制 loopback（rust-service.ts:208-218）。
- **WS**：Rust 连接使用 `/lingxi/v1/ws` 与独立握手（websocket.ts:224，`serverKind === 'lingxi-service'` 校验 websocket.ts:110；握手后仍标 `status.rustCoreUnavailable`，websocket.ts:243-247，因为 R02 Rust 只有底座）。
- 该路径不改变默认：仅当用户在 UI 手动添加远端服务器并输入 URL/凭据，或 CLI 显式带 `--url --token --runtime rust` 时才会走 Rust 协议分支。

### 1.4 打包/更新链（反证检查）

- `npm run pack/dist/dist:win/dist:linux` 均执行 `build:rust-service`（package.json:56-60），electron-builder `extraResources` 把 `dist-rust-service/${os}-${arch}/` 打进安装包 `rust-service/`（package.json:190-198，mac `binaries` 含 `Contents/Resources/rust-service/lingxi-service`，package.json:225-227）；`scripts/notarize.cjs:8,19-23` afterSign 后核对该目录 manifest 与签名。
- **打包内置 ≠ 默认切换**：资源只是被 `resolveRustBinary`（packaged 分支，rust-local-service.cjs:196-204）在**显式 rust 模式**下消费；electron-builder/notarize/launch 链没有任何地方设置 `LINGXI_DESKTOP_SERVER_RUNTIME`。auto-updater（desktop/auto-updater.cjs）无 rust 引用。
- **反证结论**：未发现任何「默认启动 Rust / 默认切换数据根」的路径；`git grep LINGXI_DESKTOP_SERVER_RUNTIME` 全仓只有默认 `node` 的读取点一处。

---

## 2. 当前 A16 gate 审计（`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`）

结构总览（行号）：
- 头注释 R1→R11 修复史：1-198；`set -euo pipefail` 与证据根：199-213；`BASE_SHA` 默认 `201584f2917a7fd96d6ea603bdeddbd420082cfe`（R02 阶段基 = 首个 R02 提交之父，line 216）。
- E0 坐标/绑定/副本纯度：227-349；E5 分类机器（SEAL_FAMILY、签名、`extract_blocks`、`failed_files`、`non_family_files`、`classify_file`）：351-729；E0s 自检 fixtures：731-1618。
- **E1 零 diff 检查：1620-1656**；E2 typecheck：1658-1661；E3 core-contracts：1663-1665；E4 双边界：1667-1671；E5 npm test + seal 族回放与分类：1673-1964。

### 2.1 逐项判定

| 检查 | 脚本位置 | 内容 | 语义判定 | 处置建议 |
|---|---|---|---|---|
| E0 祖先断言 | 227-229 | BASE 必须是 HEAD 祖先 | 中性（回放基础设施） | 保留 |
| E0 worktree 绑定 + 副本镜像 | 231-349 | tracked+untracked 全量内容 SHA 绑定；基线副本 fresh 构造 + 纯度断言 | 中性（隔离与防污染，工程质量高） | 保留 |
| E0s 分类器自检 | 731-1618 | 45 个 fixture 验证 seal 族失败分类器与可解析性谓词 | 中性（只服务 E5） | 保留（若 E5 保留） |
| **E1a surface 零 diff** | 1623-1630 | `git diff BASE -- core/ server/ desktop/ shared/ tests/ package.json package-lock.json` 必须为空 | **违背正确语义**。这正是「零 diff 即通过/有 diff 即失败」的错误逻辑：desktop/tests/package.json 的**授权修改**（R17 提前客户端接线）被一票否决。实测当前 HEAD 相对该 BASE 有 **53 个 surface 文件改动**（desktop/main.cjs、desktop/src/shared/rust-local-service.cjs、package.json、tests/cli-rust-service.test.ts 等），静态 FAIL | **删除 pass/fail 语义，降级为记录**：把 diff 清单写入证据（说明授权接线范围），不再作为门禁 |
| **E1b 绑定∩surface 为空** | 1636-1643 | untracked 绑定与同一 surface 求交必须为空 | 同上，违背（且其正则第二分支漏了 `package.json/package-lock.json` 前缀形式，只拦目录） | 同上：改记录 |
| **E1c package.json main** | 1645-1647 | `main == desktop/bootstrap.cjs` | **符合**。这是真正的「默认入口未切换」断言 | 保留 |
| **E1d 启动路径无 rust 引用** | 1649-1656 | grep `lingxi-service\|rust-target\|/rust/` 于 launch.js/main.cjs/bootstrap.cjs/server/boot.cjs 必须 0 命中 | **违背正确语义**（「主程序不得出现 Rust 服务入口引用」）。opt-in 接线必然在 main.cjs 留下 `'lingxi-service'` 字面量。实测当前 10 命中（main.cjs:973,1312,1322,2184,4714,6474,6740,6752,6760,6771，全部是 `serverNodeKind` 守卫/发布点） | **替换**为「默认分支未切换」断言（见 2.2） |
| E2 `npm run typecheck` | 1659-1661 | tsc x3 于候选副本 | **符合**（受影响回归） | 保留 |
| E3 `typecheck:core-contracts` | 1663-1665 | 严格契约检查 | **符合** | 保留 |
| E4 `check:dependency-boundaries` + `check:tool-invocation-boundaries` | 1667-1671 | 边界门禁 | **符合** | 保留 |
| E5a 候选全量 `npm test` + 可解析性 | 1674-1737 | 非零退出必须可解析、按块分类 | **方向符合**（「不新增失败」的实现载体） | 保留 |
| E5b 基线 seal 族回放 | 1739-1777 | 三文件在 pristine BASE 回放 | 符合（基线对照） | 保留 |
| E5(2) 红必须 ⊆ seal 族（整行精确匹配） | 1786-1806 | 族外红一票 FAIL | **半符合**：族硬编码为 seal 三件套是「已知基线失败族」的合法白名单，但正确语义是「不新增失败 vs 基线」，不是「只能这三种文件红」。若未来有其他既有红族会误伤 | 改为：候选红 ⊆ 基线红 ∪ 已登记既有失败族（seal 族为当前唯一登记项）；族外新红仍 FAIL（这是反回归牙齿，保留） |
| E5(3) 逐块原因分类 | 1808-1865 | seal-coordinate-lag / uncommitted-source-rejection / UNRECOGNIZED | 符合（防伪装、防吸收） | 保留 |
| E5(3b-5) 登记/对照/覆盖一致性 | 1867-1954 | 类登记、块覆盖、原因非空 | 符合 | 保留 |
| 尾注 GREEN 文案 | 1964 | "production surface unchanged vs BASE" | 文案需随 E1 语义修正改写（改为 "default entry not switched; no new failures"） | 修改 |

**当前 gate 的静态判定**：E1a 必 FAIL（53 文件 diff）且 E1d 必 FAIL（10 处 `lingxi-service` 引用）。历史上 2026-09-27 的 GREEN 运行（`artifacts/rust-tauri/R02/T08/verify-stage/a16_legacy_regression/stdout.log`，当时基线 574198916 + 未提交 T08 工作、surface 零 diff）证明：gate 一旦进入有授权接线的提交区间就永久翻红——这是 gate 语义错误，不是产品回归。

### 2.2 E1 修正后的断言集建议（替换 1623-1656 段）

1. **默认入口未切换**（保留现 E1c）：`package.json` `main == desktop/bootstrap.cjs`；另断言 `desktop/bootstrap.cjs:187-195` 仍按 isPackaged 加载 `main(.bundle).cjs`（grep 结构断言）。
2. **运行时选择器默认值断言**（新，替代 E1d）：在候选副本内执行
   `node -e "const m=require('./desktop/src/shared/rust-local-service.cjs'); if(m.rustDesktopEnabled({})!==false||m.rustDesktopEnabled({LINGXI_DESKTOP_SERVER_RUNTIME:'node'})!==false||m.rustDesktopEnabled({LINGXI_DESKTOP_SERVER_RUNTIME:'rust'})!==true)process.exit(1)"`
   ——证明不设 env 即 Node、只有显式 `rust` 才 Rust。
3. **无默认注入断言**（新）：grep 断言 `LINGXI_DESKTOP_SERVER_RUNTIME` 不出现在 package.json（scripts/build/electron-builder 段）、`scripts/launch.js`、`desktop/bootstrap.cjs`、electron-builder 配置（package.json `build` 段）与 `scripts/notarize.cjs`——即没有任何链路替用户设置 rust。
4. **分支可达性断言**（新，替代「零 rust 引用」）：断言 main.cjs 中 `startServer()` 的 rust 分支被 `rustDesktopEnabled()` 守卫（如 grep `if (rustDesktopEnabled())` 在 `startRustServer()` 调用之前），且 `startServer` 的 Node 主体（`server-info.json` 处理）仍存在。
5. **CLI 默认断言**（新）：`cli/args.ts` 解析默认 `runtime==='node'`（可用 `node -e` 直接调 `parseCliArgs([])` 于副本，或 grep `runtime: "node"` 默认值行）。
6. **数据根不双写断言**（新）：grep 保留 `RUST_DESKTOP_NODE_SERVER_INFO_PRESENT` 守卫（main.cjs:1333）与 rust `paths.rs` 的 `RUNTIME_DIR_NAME = "lingxi-service"` 布局（或引用其单测 `tests/desktop-rust-local-service.test.cjs` 已覆盖的事实）。
7. **surface diff 降级为证据记录**（原 E1a/E1b 改造）：`git diff --name-only BASE -- <surface>` 与绑定∩surface 清单写入 evidence（供审计查看授权接线范围），不再 FAIL；如需兜底，可仅对 `package.json` 的 `main`/入口相关键与 `desktop/bootstrap.cjs` 保持强断言（已含于 1）。
8. **补 `npm run build:renderer`**（E4.5 或 E2.5，新）：当前 gate 未跑渲染构建；客户端接线改动（server-connection/websocket/StatusBar 等）使其成为受影响回归的必要项。

E5 的「候选红 ⊆ 基线红 ∪ 登记既有族」调整见上表 E5(2) 行。其余（E0/E0s/E2/E3/E4/E5 机器）不动。

---

## 3. 受影响回归命令映射表

| 任务书/指令侧名称 | package.json 实际 script（行号） | 实际命令 | 当前 gate 是否执行 | 备注 |
|---|---|---|---|---|
| `npm run typecheck` | `typecheck`（package.json:39） | `tsc --noEmit && tsc -p tsconfig.node.json && tsc -p tsconfig.test.json` | 是（E2） | 无改名 |
| `npm run typecheck:core-contracts` | `typecheck:core-contracts`（:40） | `node scripts/check-core-contracts-strict.mjs` | 是（E3） | 无改名 |
| `npm run check:dependency-boundaries` | `check:dependency-boundaries`（:42） | `node scripts/check-dependency-boundaries.mjs` | 是（E4） | 无改名 |
| `npm run check:tool-invocation-boundaries` | `check:tool-invocation-boundaries`（:41） | `node scripts/check-tool-invocation-boundaries.mjs` | 是（E4） | 无改名 |
| `npm run build:renderer` | `build:renderer`（:35） | `vite build --config vite.config.ts` | **否（缺）** | 建议补入 gate |
| `npm test` | `test`（:67） | `vitest run --exclude …` | 是（E5a） | 无改名 |
| `npm run cli` | `cli`（:43） | `node scripts/launch.js cli` | 否 | 旧入口冒烟可用（可选） |
| `npm run server` | `server`（:44） | `node scripts/launch.js server` | 否 | 同上 |
| （Rust 侧参考） | `build:rust-service`（:52）/`verify:rust-service-package`（:53） | `node scripts/build-rust-desktop-service.mjs [--verify]` | 否 | opt-in 链交付件，非 A16 必需 |

**已知基线失败族**（历史证据，非 R02 引入）：
- seal 三件套：`tests/post-verification-audit-seal.test.ts`、`tests/round2-delivery-evidence.test.ts`、`tests/round3-delivery-evidence.test.ts`。
- 根因：`.sync-audit/verified-source-sha.txt` 冻结坐标 `ab4f228155dd4246734e6c4c45d7288072361f3c`（本审计确认其为 HEAD 祖先）之后的授权提交被守卫列为「非审计改动」——封印工作流的坐标滞后，属 PROGRESS.md 封印流程范畴，不是 R02/A16 行为回归。历史上 2026-09-27 A16 GREEN 运行也只登记了这三个文件为既有红族。
- 本审计未重跑全量 `npm test`（只读审计、耗时长）；「当前除 seal 族外是否还有红」以修正后 gate 的 E5 实跑为准。

---

## 4. 总结论

1. **PRODUCTION DEFAULT = Node/Electron**（证据链：package.json main → bootstrap.cjs:187-195 → main.cjs:1437-1441 → rust-local-service.cjs:186-192 默认 `node`；`LINGXI_DESKTOP_SERVER_RUNTIME` 全仓唯一读取点且无人默认注入）。
2. **显式 Rust preview 链完整且互斥**：env（桌面）/`--runtime rust`（CLI）显式选择；不选择不启动、不创建运行时目录；Rust 与 Node 数据根记录隔离（`{home}/lingxi-service/` vs `{home}/server-info.json`）且双向拒绝同宅共存（main.cjs:1330-1334、server-runner.ts:182-191）。
3. **远端 Rust 链存在且用户显式发起**（渲染层手动连接 server-connection.ts:333-391；CLI `--url --token --runtime rust`）。
4. **打包链内置 Rust 资源但不切默认**（extraResources + notarize 校验；无任何 env 注入）。
5. **当前 A16 gate 语义错误在 E1a/E1b/E1d**（零 diff + 零 rust 引用），静态必 FAIL 于授权接线后的 HEAD；E1c、E2-E5 方向正确。修正方案见 2.2（默认未切换断言集 + surface diff 降级记录 + 补 build:renderer + E5 族白名单语义微调）。
