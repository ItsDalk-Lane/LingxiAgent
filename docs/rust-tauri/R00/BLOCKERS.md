# R00 阻塞登记（BLOCKERS.md）

生成：`docs/rust-tauri/R00/r00_t07_build_map.py`｜基准 HEAD `8b153b1031bbb01204375b08e9caaa891397d7a5`｜配套账本 `ACCEPTANCE_MAP.json`。
状态语义遵循 01 通用约束 §6：BLOCKED 表示已到执行时点但缺凭证/平台/授权；
未来阶段场景保持 NOT_STARTED/SPECIFIED_NOT_EXECUTED，其外部依赖在此预登记（anticipated），
到对应阶段仍缺条件时转为 active BLOCKED，不预写 FAIL、不伪造 PASS。

## 1. 当前 ACTIVE 阻塞（阻止 R00-T07 放行的项）
- 无。R00-A13/A14 已在本机隔离环境真实执行（见 ACCEPTANCE_MAP.json results 与 artifacts/rust-tauri/R00/T07/）。

## 2. 条件授权（未授权不算失败，也不算通过）
- **BLK-RELEASE-AUTH**：远程发布未获用户授权；场景 R11-A14 状态 NOT_RUN_UNAUTHORIZED；最晚消除：R11（授权后激活；未授权不阻塞技术交付）

## 3. 本轮声明未执行（NOT_RUN_THIS_ROUND，已有冻结口径）
- **BLK-LONGRUN-G1**：长时资源增长（2h/1000 任务）G1 侧未测（T06 登记 NOT_RUN_THIS_ROUND，阈值已冻结）；最晚消除：R10

## 4. 预登记（ANTICIPATED）：未来阶段外部依赖，按关键词保守绑定

| 阻塞 ID | 原因 | 最晚消除阶段 | 绑定场景数 | 绑定关键词 |
|---|---|---|---|---|
| BLK-CREDENTIALS | 缺真实供应商凭证/真实测试账号授权（LIVE 项） | R10 | 2 | 真实供应商、测试账号、LIVE项 |
| BLK-PLATFORM | 缺目标平台真机（规格明示真实/各/四组目标 OS） | R10 | 7 | 真实目标操作系统、真实安装的目标OS、各目标操作系统、四组目标平台 |

逐场景绑定明细在 `ACCEPTANCE_MAP.json` → `blockers.anticipated[].scenario_ids`（可查询、可复核）。

## 5. 账本状态分布（构建时点）

| 维度 | 计数 |
|---|---|
| 场景 kind base | 200 |
| 场景 kind supplemental | 736 |
| 场景 kind t07_added | 16 |
| ledger_status NOT_RUN_UNAUTHORIZED | 1 |
| ledger_status NOT_STARTED | 183 |
| ledger_status PASS | 32 |
| ledger_status SPECIFIED_NOT_EXECUTED | 736 |
