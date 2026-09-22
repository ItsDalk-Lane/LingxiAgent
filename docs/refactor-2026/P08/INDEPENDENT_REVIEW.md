# INDEPENDENT_REVIEW — P08 独立负向复核与最终范围核对（P08-T06）

日期：2026-09-22｜复核者=本阶段执行子代理（全新上下文，无前序会话记忆）；方法=仅持任务书（P00/P08 + 01 通用约束 + 92 模板）、真实源码树（HEAD `674b0151f`）与各阶段原始日志（command-log.jsonl / 样本 / 复验收报告），不以实现者总结为唯一输入。本报告不写"绝对无问题"——§4 列出已尝试反例与仍未验证边界。

## 1. 反例尝试与结果（全部真实执行，非推演）

| # | 威胁类别 | 反例构造 | 实际执行与结果 | 证据 |
|---|---|---|---|---|
| C1 | 权限旁路（产物形态） | 无凭证 GET /api/health 打真实启动的**打包产物 server**（seed 提取树，非开发环境） | 403 拒绝；带 token 200——保护不依赖开发环境（A10） | logs/packaged-server-smoke.json（7/7） |
| C2 | 凭证伪造 | 错误 Bearer token（"hanа-forged"）列会话 | 403 + 会话列表零变化（N1） | logs/negative-probes.json |
| C3 | 身份串线/跨域资源 | 伪造 sessionId+agents 目录外 sessionPath 调 pin | 404（sessionId→manifest 解析先行失败，fail-closed；不泄漏路径有效性）+ 零副作用（N2） | logs/negative-probes.json |
| C4 | 输入校验绕过 | 合法 JSON 但字段类型错误（agentId:12345）；截断 JSON | 400 invalid_field_type / 400，均零副作用（N3 + product-regression A07 负向 5 例） | logs/negative-probes.json、product-regression.json |
| C5 | 升级窗口数据丢失 | 会话 JSONL 尾部截断 120 字节（模拟 append 中途崩溃）后再启动 | 其余 3 会话完好、启动无半激活（server-info.json 仅就绪后写出）、受损文件未静默清空其余数据（A09） | logs/upgrade-rollback-drill.json FAULT 节 |
| C6 | 旧程序覆盖新数据 | 旧版本代码（8037fae7a）在升级后 HOME 再启动并新建会话 | 4 会话且新版本写入的 3 个 ID 全部保留（A09） | logs/upgrade-rollback-drill.json ROLLBACK 节 |
| C7 | 工具执行旁路 | 复核 executeCanonical AST 白名单（本轮复跑）+ 全仓 ToolInvocationGateway 唯一构造点 grep | 边界检查 exit 0；唯一定义+唯一构造点（engine.ts:4119） | P08-T01-tool-invocation-boundaries、FINAL_CALLSITE_MATRIX second_fact_owner_scan |
| C8 | 隐藏旧回退 | 本轮生产 diff 全集逐文件核查（92c6646c5..HEAD，36 文件）+ 配置/flag 扫描 | 零 feature flag 新增、零默认走旧实现、零失败静默回退（LEGACY_EXIT_LEDGER §0/§4） | git diff + LEGACY_EXIT_LEDGER.md |
| C9 | 观测旁路复发 | P01 门禁 dependency-boundaries + pi-sdk-import-boundary 复跑 | 双绿（SDK 直导/深路径零回潮） | P08-T02-dependency-boundaries 等 |
| C10 | 假优化/预算破坏 | 引用 P07 既有反例资产（bench-compare --selftest 假优化判 INVALID；P06 预算 A/B/C ≤ 基线 + golden 字节锁定） | 本轮 P08 未触碰性能与提示词面（生产 diff 无相关文件）→ 既有锁定继续有效，未复测数值（无改动即无回归面，如实注明） | P07/P06 交付物；P08 生产 diff 清单 |

## 2. 本阶段交付物抽查复核（复核者重算，非转抄）

- 全量 npm test 结果与 F1 基线逐名比对：4 红一致（audit-seal 1 + round2 2 + round3 1）；14797 绿与 P07 完全一致（P08 未新增测试文件，绿数不变自洽）。**复核者重读原始 stdout 首末行确认**（P08-T04-full-test.out）。
- f1-f12 patch 还原哈希 25fb315f 复核通过（F5 副作用闭环）。
- 打包产物：seed kit 三个 sha256 与 .sig 独立重算一致；ad-hoc 签名状态独立 codesign 复核（Signature=adhoc）；skills2set 残留清理随包生效（提取树内无 lingxi-plugin-creator，5 技能目录）。
- P07-F-B 修复行为验证：trim 微基准真实复跑，留档样本哈希前后一致（94243baf0edc），新输出落 run-id 后缀路径。
- P06/P04 证据链：更正后条目哈希 shasum -c 全 OK（P04 70 条 / P06 11 条 / P07 logs 147 条。P08-FIXR1 更正：P06 原记 12 条，实测 11 条哈希行，12 为含头注总行数）。

## 3. 最终范围核对（任务书 T06.4）

| 核对项 | 结论 | 依据 |
|---|---|---|
| 撤回的专用子代理目录/强制分发改造未重引入 | 通过 | 生产 diff 无相关文件；现役仅可选实验开关 proactive_delegation（beta 默认 false，P00 已登记为产品行为） |
| 独立知识研究未复活 | 通过 | research 表族保持仅建表兼容（P05-2 决策）；零新增运行时入口；本轮无人触碰 |
| 本地模型管理子系统未恢复 | 通过 | 全域 grep 仍零痕迹；Ollama 仅为模型接入（非管理子系统） |
| UI 布局/品牌未改 | 通过 | renderer 生产源码本轮零 diff（仅测试文件新增）；build:renderer 绿 |
| 权限/人格/MOOD/观测分组/知识来源/文件交付行为未变 | 通过 | 用户采纳行为面的生产 diff = P02 taskId 铸造（行为等价迁移，P02 验收）+ P06 schema-validator 字段化反馈；其余为零；32/32 产品回归探针 + 14797 套件绿 |
| 未新增平行 Agent 循环/网关/总管/凭证体系 | 通过 | FINAL_CALLSITE_MATRIX second_fact_owner_scan + grep 复核 |
| 未以新版本号/指纹掩盖未验收 | 通过 | 版本保持 0.1.42；pinned-keyset 未动；指纹守卫 170 watched 零触碰 |

## 4. 发现分类

**本轮验收缺口（须随授权关闭）**：无新增（本阶段执行中发现的 5 个探针形状问题均为复核工具自身缺陷，已修复并留首败链，非产品缺陷）。

**既有范围内阻塞（继承，如实登记不关闭）**：
1. F1 封印 4 红（治理流程，需候选提交存在后按 PROGRESS.md 推进——本阶段无提交授权）。
2. 四平台 CI 候选 SHA 证据（ci.yml 仅 PR 触发；待触发清单见 FINAL_ENGINEERING_CHECKS）。
3. P04-T07-2 真供应商冒烟 / P06 真模型行为评测（无凭证/费用授权）。

**范围外建议（不自动追加）**：
1. H1 stream-store trim O(n)/append 热点（P05 锁定面；最小修复方案已成文，需显式解锁授权）。
2. P05 C1 use-stream-buffer streamId 闸门（消息语义面，修复时 it.fails 反例转红提示）。
3. R10-09 补丁重写测试行为改造（审计证据链测试，需独立授权）。
4. lib/task-registry.ts 存量 any 风格 strict 化（独立质量专项）。

**未验证边界（诚实清单，不写无问题）**：
- 真实供应商/付费模型、真实 Bridge 平台账户、桌面 GUI 层（弹窗/卡片/截图）、Electron 窗口级启动、Windows/Linux/macOS-x64 实机、DMG/NSIS/AppImage 安装器形态、产物级 OTA 列车、真实用户 HOME 数据。以上均已在对应矩阵标 BLOCKED/NOT_ATTEMPTED 并给出原因与解锁条件。

## 5. 复核结论

在本报告抽样与真实反例范围内：P08 各任务交付物与原始日志相互印证，未发现权限放宽、跨主体污染、数据丢失、重复副作用或隐藏旧回退。总体完成度结论（含 BLOCKED 项枚举）见 P08_REPORT.md（§结论 + §差异与限制）/ FINAL_RESULT.json——受限项不写成通过。（P08-FIXR2 更正：原引 FINAL_REPORT.md 系任务书 T07 建议文件名，实际不存在，该职责由 P08_REPORT.md 承担。）
