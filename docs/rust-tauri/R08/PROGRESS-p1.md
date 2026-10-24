# R08-P1 客户端数据纪元对账 — 台账

- 执行人：t02-a04（Lead 直派，独立于 T05 主实现）
- 日期：2026-10-23（Asia/Shanghai）
- 结论：**PASS** —— 客户端握手校验已就绪，是 `t02-core` 切换 Rust 侧 `R08_CUTOVER` 常量的前置绿灯。

## 任务范围（逐字域）

`desktop/src/react/services/websocket.ts` + 一个新测试文件 + 本台账；其余禁触。
（任务书写的是 `desktop/src/react/lib/websocket.ts:129`；该路径不存在，按行号 34/129
grep 定位到同角色的真实文件 `desktop/src/react/services/websocket.ts`。）

## 取证（改前）

- `isRustWireServerHello()`（现 :122-140）：hello 全字段校验，其中纪元条件原为
  `hello.dataEpoch === DATA_EPOCH`（严格相等，`DATA_EPOCH` 来自
  `shared/contract-versions.ts`，当前值 1）。
- 调用链两处：
  1. `services/websocket.ts` `onmessage` 首帧握手（现 :398-417）：不通过则
     `failRustSocket('status.wsHandshakeFailed', 4409, 'incompatible Rust wire handshake')`。
  2. `components/chat/RustMoodPanel.tsx:148`：情绪连接版本闸（自动继承新接受集，未改其代码）。
- 拒绝呈现面：`wsFailureReasonKey`（i18n key，入 store）+ WS close code/reason。
  本任务不改 i18n（新增 locale key 出域），方向区分落在 close reason 与导出判定函数上。

## 最小改动（websocket.ts，+34/-3）

1. 新独立常量 `RUST_ACCEPTED_DATA_EPOCHS: readonly number[] = [1, 2]` —— **显式枚举**，
   禁 `>=` 通配；注释钉死 R08/T05 背景与「窗外一律拒绝」。
2. 纪元条件：`hello.dataEpoch === DATA_EPOCH` → `RUST_ACCEPTED_DATA_EPOCHS.includes(...)`。
   非数字/NaN/undefined 经 `includes` 自然拒绝，语义与原严格相等一致。
3. 新导出 `rustWireHelloDataEpochDirection(value)`：接受集内 → `null`；整数低于集 →
   `'too_old'`；整数高于集 → `'too_new'`；非整数/缺失 → `null`（归通用不兼容，不归方向）。
4. 握手拒绝路径按方向分流 close reason（code 仍 4409，reasonKey 仍
   `status.wsHandshakeFailed`，非纪元拒绝的通用理由串逐字不变）：
   - too_old：`Rust data epoch too old for this client (accepted: 1,2)`
   - too_new：`Rust data epoch too new for this client (accepted: 1,2); upgrade the client`
     （对应任务书 A11：已升级数据目录 → 提示升级客户端而不是继续写入）
5. 移除已不被引用的 `DATA_EPOCH` import（`shared/contract-versions.{ts,json}` 本体未动）。

无静默降格：窗外纪元仍 4409 拒绝、仍 `status.wsHandshakeFailed`、绝不进入 connected。

## 新测试

`desktop/src/react/__tests__/services/r08-p1-data-epoch-hello.test.ts`（13 用例）：

- 纯校验钉死：epoch=1 接受、=2 接受、=0 拒+too_old、=3 拒+too_new；
  缺失/1.5/'2'/null 拒且不归方向（通用不兼容）。
- 握手集成（FakeSocket + 与 R02 同形 mock，另将 session-connection-owner /
  rust-media / rust-workbench / rust-automation 置为惰性，以隔离 R02 基线红的
  stub 形状根因）：epoch=2 与 epoch=1 均进入 connected 且 failureReasonKey 清空；
  epoch=0/3 均 4409 + 对应方向 reason、wsState 保持 reconnecting、永不 connected；
  两方向 close reason 字符串互不相同。

## 证据

| 证据目录 | 内容 | 结果 |
|---|---|---|
| `artifacts/rust-tauri/R08/p1-baseline-01` | 改前 R02RustWebSocket.test.ts | 1 红 / 5 绿（红=`composerOpened` 断言 :85，epoch 无关） |
| `artifacts/rust-tauri/R08/p1-epoch-01` | 改后新测试文件 | **13/13 绿** |
| `artifacts/rust-tauri/R08/p1-r02-regression-01` | 改后 R02RustWebSocket.test.ts | 1 红 / 5 绿，**与基线逐行同根因**（红绿状态未变） |
| `artifacts/rust-tauri/R08/p1-adjacent-01` | websocket.test.ts + 新测试 | **15/15 绿** |
| `artifacts/rust-tauri/R08/p1-typecheck-01` | `npm run typecheck`（tsc×3） | exit 0 |

注：
- p1-epoch-01 的 `sourceBindingsStable:false` 为运行期间队友 Rust 文件
  （`legacy_import_runner.rs`、`main.rs`）漂移；本任务两文件哈希稳定。其余证据轮
  `sourceBindingsStable:true`。
- `websocket.test.ts` 只测未触碰的两个纯函数（resume targets / terminal snapshot），
  唯一耦合是模块可导入性，改后绿即覆盖，故未单列改前基线。

## 禁触核对（git status 逐文件）

- `desktop/src/react/__tests__/services/R02RustWebSocket.test.ts`：**未改**（红绿状态改前后一致，证据同上）。
- `shared/contract-versions.ts` / `shared/contract-versions.json`：**未改**。
- 本会话改动面仅此三项：websocket.ts（M）、新测试文件（??）、本台账（??）。
