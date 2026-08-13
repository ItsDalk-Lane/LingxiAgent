# PROGRESS — 用量统计独立成页 + 图表悬浮提示

## 终态：完成（未触 8 轮上限）

## 目标
把「用量统计」从供应商页拎出，做成设置左侧导航独立一页（夹在 providers 与 media 之间），并给各图表加跟随鼠标、实时显示数值的悬浮提示。

## 基线数（2026-08-13）
- `npx vitest run desktop/src/react/__tests__/settings/`：30 文件 174 用例全过，skipped=0
- `npx vitest run desktop/src/react/settings/__tests__/`：6 文件 23 用例全过，skipped=0
- `npm run typecheck`（tsc x3）：绿
- locale key 完整性校验测试：grep 无，不纳入验收

## 最终数
- `desktop/src/react/__tests__/settings/`：31 文件 179 用例全过（+1 文件 +5 用例：UsageTab.test.tsx 4 条、ProvidersTab.test.tsx 1 条），skipped=0
- `desktop/src/react/settings/__tests__/`：6 文件 23 用例全过（= 基线），skipped=0
- `npm run typecheck`：绿
- 反向验证：UsageCursorTip 渲染出口改 return null → UsageTab.test 2 条提示用例如期变红；还原 → 4 条全绿

## 最大风险（实际发生与化解）
1. SettingsContent.test.tsx（白名单外）逐模块 mock tab：未 mock UsageTab，但它从不把 activeTab 设为 usage，真实 UsageTab 不会被渲染；UsageTab 保持薄壳，模块级 import 无副作用 → 全程绿。
2. UsageLedgerSection.test.tsx（白名单外）用 `getAllByTitle(/·/)` 数原生 title 探针（7/30）。任务 2 要求 `.usage-day` 删 title。化解：title 从 `.usage-day` 移到其子元素 `.usage-day-label`（每柱仍恰好 1 个，计数不变；柱体本身不再触发原生提示，光标提示是唯一柱体悬浮层；`.usage-day` 改挂等价 aria-label）。属「靠调整白名单内实现修好」，已在 BLOCKED.md 记为待裁决观感项。
3. settings-primitives-contract.test.ts（白名单外）对 tabs/ 下 `style={{` 计数棘轮 ≤48。初版 UsageCursorTip 用 inline style 定位导致 49。化解：改 ref 直写 DOM style（第 2 轮失败是注释里含 `style={{` 字面量被正则计入，改写注释后通过）。

## 记录
- settings.providers.subtab.usage 变死 key：按已拍板事项保留不删（5 文件同步删除无收益）。
- 任务 1（独立成页）：nav TAB_ITEMS 插入 usage（柱状图 icon）、SettingsContent 注册 TAB_COMPONENTS/TAB_TITLE_KEYS（标题复用 settings.usage.title）、新建薄壳 UsageTab、ProvidersTab 移除 usage 子页（api/models 不动）、5 locale 加 settings.tabs.usage。一轮通过（新断言 role button→tab 属当轮内修正）。
- 任务 2（悬浮提示）：新建 UsageCursorTip（portal→body、position:fixed、pointer-events:none、data-testid="usage-cursor-tip"、useUsageCursorTip 每图表一份状态、enter 显示/move 跟手+贴边翻转/leave 立即卸载、进入向 fade+scale）；ModelOrbit 每环段包 `<g>` 挂事件、SplitRing 环段挂事件、DailyBars 每根 .usage-day 挂事件；RingCircles 保持纯展示。提示文案全部复用 settings.usage.totalTokens/cacheRead/uncached/requests/cacheHitRate。

## 2026-08-13 真机反馈修复
用户在桌面 app 实测悬停无提示。根因：提示 portal 到 `document.body`，原 z-index 1000 低于设置 modal 壳（`SettingsModalShell.module.css` z-index:1800）与设置内 fixed 浮层（9998/9999），提示被压在设置面板下面不可见；jsdom 不断言层叠所以测试全绿没暴露。修复：`.usage-cursor-tip` z-index 提至 10000；顺手把定位从 useEffect 改 useLayoutEffect，避免首帧出现在左上角再跳变。复跑：两目录 202 用例全绿、typecheck 绿。

## 2026-08-13 用户拍板：彻底删除原生 title
经用户确认，`.usage-day-label` 上的兼容 title 已移除，DailyBars 不再有任何原生 tooltip；
`UsageLedgerSection.test.tsx`（经用户授权修改）的周/月窗口探针从 `getAllByTitle(/·/)` 换成
`getAllByLabelText(/·/)`（7/30 计数不变，语义等价——aria-label 承接了原 title 的信息）。
复跑：两目录 202 用例全绿、typecheck 绿。BLOCKED.md 清空为「无」。
