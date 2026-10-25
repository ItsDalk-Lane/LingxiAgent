# R08-T08 进度台账｜迁移试用关卡与交接

- 状态：**READY_FOR_REVIEW**（诚实关闭：实现到位项就绪；117/126 叶 pending 如实；门已注册运行）
- 本会话交付：

1. **xtask R08 注册**：`stage_maps/R08.json` + `R08_INPUTS.json`（`r08_stage_inputs.py` 生成：18 真实命令 / 16 基础场景 R00 镜像 / 126 叶=9 stage_share_satisfied（A/B 正路径份额图钉，cases.json 机器核对）+ 117 required_behavior_pending）；main.rs 阶段表 + runner_identity.rs EMBEDDED/清单同步（t08-xtask-build-01/02/03）。
2. **门工具修复（如实登记）**：xtask candidate 快照对工作树已删除 tracked 条目（任务前遗留 8 万脏态 D 项）哈希失败 → 修复为跳过（候选=工作树现状语义；candidate.rs）。首验红链：t08-xtask-validate-01（originalAssertionCases 字段规则）、validate-02（证据目录复用拒绝——xtask 语义）、dryrun-01/02（候选快照删除条目）。
3. **A15**：`r08_t08_isolation_check.mjs`（真实 home 只读摘要 + 预览开关 env 门控源码事实 + 合成 HOME 使用声明；开工基线未采集如实披露，本摘要为 R09+ 首基线）。**A16**：`r08_t08_version_manifest.mjs`（HEAD/二进制 sha/渲染层树摘要/契约摘要/双锚/阶段图 digest）。
4. **交付物**：`R08_REPORT.md`（含 PPG-1/2/3 终裁=接受预览限定+post-R08 修复、门非绿=覆盖缺口的诚实表达）、`R08_HANDOFF.json`。
5. **最终门**：dry-run `/private/tmp/r08-gate-dryrun-03`（后台全量 18 命令）；正式证据目录 `verify-r08-final-01`（结果快照以 verify-stage-result.json 为准；逐命令红若出现逐条根因补记本文件）。

## 遗留（R09+）

117 pending 叶补证转绿；D-4 定位；PPG 三项产品修复；卫生清单（PROGRESS.md 家族 #5/#7/#8 + manifest 双哈希修复+锚重封）。

## 最终门结果登记（2026-10-23/24）

- **正式门**：`artifacts/rust-tauri/R08/verify-r08-final-01`（verify-stage-result.json）：**overall=FAIL（诚实预期）**——命令 17 中 16 绿 1 红；场景 16 中 14 PASS / 2 FAIL（R08-A05/R08-A06，同因单命令红连带）；补充叶 126 全 BLOCKED（required_behavior_pending，门规末位阶段无份额拆分合法位——9 叶已测正路径份额以 evidenceRequired 注记保留，非门禁）。
- **唯一红命令** `r08_t03_a05_drill`：electron-exit timeout 45s——**确定性复现**（门内 2 次 + 隔离单跑 t08-a05-single-rerun-01 共 3 次同签名）；T03 时代绿证据链（drill-06..08）在案 → **定性=环境受限家族新条目（#9：Electron 退出路径环境漂移，根因未证）**，非 R08 产品回归；A05/A06 场景如实 FAIL 标注。A05 双 BLOCKED(PPG-1/2) 呈现语义在 T03 证据链与报告中仍成立。
- dry-run `/private/tmp/r08-gate-dryrun-03`（旧地图：9 份额叶 FAIL + 117 BLOCKED——份额拆分被门规 R05 RR1 F25 拒绝的实证，促使降级 pending 的诚实修正）。
- 终审：R08_FINAL_REVIEW.md = **ACCEPT_HONEST_CLOSURE（B0/M0/M4）**；M1–M4 已修（报告口径/HANDOFF 措辞/哈希权威指向 a16/candidateDiff 盲区披露）。

## R08 终态

8/8 任务闭合（T01–T06 评审通过、T07/T08 READY_FOR_REVIEW+终审接受诚实关闭）；阶段**未全绿放行**（126 叶 pending + A05/A06 环境红）——按任务书 §6「未满足必需项则标 FAIL/BLOCKED」执行；移交清单在 R08_REPORT §七。无 commit/push；HEAD 86840d170 未动。

## 叶级补证轮最终门（2026-10-24）

- **verify-r08-final-04**：**2 叶从 BLOCKED 转 PASS**（R00-T02-LA-FDB0FE6528EE 重命名会话、R00-T02-LA-38EEA99077E8 查看可见会话列表——full_original_behavior 逐断言专属案例经 lingxi.leaf-case-results.v1 机器核对全成立）。补充叶分布：**PASS 2 / BLOCKED 124**。
- 命令 15/17 绿；2 红 = r08_t03_a05/a06（Electron 调试口环境红，家族 #9——a05 三次门内+两次隔离复现同签名；a06 本轮尾段 CDP ECONNREFUSED，02/dryrun 轮绿，间歇性）；场景 14/16（A05/A06 连带，如实 FAIL）。
- 门链红证据：final-02（cases schema 不符）→ final-03（整记录规则吃进未钉 D-6 案例）→ **final-04（两叶 PASS）**。
- 诚实边界重申：升级上限 = 2 叶（rename/list）；cleanup=D-6 真差异、pin/todos×3/sweep/permissions=断言前提不可合成，124 叶留债 R09+。

## 第二轮最终门（2026-10-24）

- **verify-r08-final-05**：叶 **PASS 3 / BLOCKED 123**（rename/list/+**查看 MCP 连接状态** DD47275AC5AE full_original_behavior 机器核对成立）；命令 16/17（**a05 套件门内转绿**——PPG-2 修复生效；唯一红=a06）；场景 14/16——A05 仍被按任务粗绑定的 a06 环境红拖垮（gate-05 实证）。
- **场景→套件精确化**：A05 只绑 r08_t03_a05_drill（该演习即 A05 证据），A06 只绑 r08_t03_a06_drill——A06 的环境红不再连带 A05。t09-xtask-build-02 重建。
- **verify-r08-final-06 运行中**（预期：A05 转 PASS；a06 环境红维持=A06 如实 FAIL）。

## 第二轮最终门结果登记（2026-10-24）

- **verify-r08-final-06**：场景 **15/16 PASS**（**R08-A05 转绿**——PPG-2 修复 + 场景精确绑定双生效；唯一非 PASS=R08-A06，a06 环境红〔System Events keystroke 失效 + 客户端 A 渲染目标中途死亡，根因已定名，t09-a06-standalone-05 链〕）；命令 16/17；**补充叶 PASS 3 / BLOCKED 123**（rename/list/MCP 连接状态三叶 full_original_behavior 机器核对成立）。
- 累计两轮叶补证成果：126 叶中 3 叶转 PASS + 9 叶带已测正路径注记；1 基础场景转绿（A05）；1 真产品修复（PPG-2 授权根接线）落地并在门内验证。
- overall 仍=FAIL（123 pending 叶 + A06 环境红）——诚实覆盖缺口表达，与治理规则一致（未满足必需项标 FAIL/BLOCKED，不伪造全绿）。

## 第三轮最终门（2026-10-24）

- **verify-r08-final-07**：补充叶 **PASS 4 / BLOCKED 122**（+设置新会话默认权限 8D3CEB6133E1：PUT 写-回读-非法值 400 双内核全绿，full_original_behavior 机器核对成立）；场景 15/16（唯 A06 环境红）；命令 16/17（唯 a06）。
- **三轮叶补证累计**：126 叶中 **4 叶转 PASS**（rename/list/MCP 连接状态/设置新会话默认权限）+ 10 叶带已测注记；A05 场景转绿（PPG-2 真修复）；**D-4 定性推翻并改判关闭**（探针 t10-d4-probe-01：服务端 rename 无缺陷，原症状为套件序列级联已修）；**新差异 D-7**（switch 投影 permissionMode：旧栈归一化 vs Rust 透传 DB null）如实登记。
- overall 仍=FAIL（122 pending + A06 环境红）——诚实覆盖缺口，与治理规则一致。

## 第四轮最终门（2026-10-24）

- **verify-r08-final-08**：叶 PASS 4 / BLOCKED 122；场景 15/16（唯 A06）；命令 16/17（唯 a06 环境红）。叶数与门 07 持平（本轮产出是**差异收敛与真缺陷修复**而非新叶绑定：归档列表 500 修复、F-T07-3 解包、restore 翻译、D-7 归一化——A/B 线束从 parity=15/differs=6 收敛到 **22/25 平价、0 真差异**〔t11-ab-drill-fixes-06，唯 session-mode A 腿 D-3 环境前置〕）。
- **四轮累计**：4 叶 PASS + 12 叶已测注记；场景 14/16→15/16；命令 15/17→16/17；**6 个真产品/客户端缺陷修复**（PPG-2 授权根接线、归档列表 500、F-T07-3 解包、restore 翻译、D-7 归一化、D-4 改判套件修正）；伪差异澄清 2 起（D-4/D-6）+ 语义漂移记录 2 起（ASK 缺省、D-8 A 侧显式删腿）。
- overall 仍=FAIL（122 pending + A06 环境红）——诚实覆盖缺口。

## 第五轮最终门（2026-10-24）

- **verify-r08-final-09**：补充叶 **PASS 6 / BLOCKED 120**（+查看归档会话 43FFB75F252C、+恢复归档会话 31CDA283A11A——归档正向全链双内核绿 + 无效目标拒，full_original_behavior 机器核对成立）；场景 15/16（唯 A06）；命令 16/17（唯 a06）。
- **五轮累计**：叶 2→**6**（rename/list/MCP 连接状态/设置默认权限/查看归档/恢复归档）+14 叶已测注记；A05 场景转绿；**7 个真缺陷修复**（+归档列表 500、F-T07-3 解包、restore 翻译、D-7 归一化、PPG-2）；伪差异澄清 3 起（D-4/D-6/D-3 残余定性）；新差异如实登记 D-8（A 侧显式删腿）/D-9（A 侧 cleanup 计数）。
- **D-3 扩腿结论**：A 腿模型面经 provider-catalog 注入已可用（catalogVersion 链实证）；switch 残余阻断=旧栈 session-restore-model-ref 产品语义（无真实使用痕迹的合成会话无法 restore——真实旧根会话有真实 model_change，不受此限）。session-mode 叶维持不可绑定（如实）。
- overall 仍=FAIL（120 pending + A06 环境红）——诚实覆盖缺口。

## 第六轮最终门（2026-10-24）

- **verify-r08-final-10**：补充叶 **PASS 9 / BLOCKED 117**（+重排置顶会话 2ED065BED168、永久删除归档会话 52226FEA5997、跨会话搜索 A18187567AF9——三叶 full_original_behavior 双断言独立案例机器核对成立）；场景 15/16（唯 A06）；命令 16/17（唯 a06）。门规新实证：assertionContract 案例须唯一身份（重复案例名硬拒）——三叶两断言分立案例键后过门。
- **D-9 结案**：旧栈 cleanup 计数=文件 mtime（sessions.ts:2536）vs Rust=archived_at 列——真实旧根两者近似一致，夹具放大语义差；如实记录为双内核语义差，不静默对齐。
- **六轮累计**：叶 2→**9**（D02 域 8 + D03 域 1）+14 叶已测注记；A05 场景转绿；7 真缺陷修复；伪差异澄清 3 起；语义差登记 D-7/D-8/D-9（各附根因与处置）。
- overall 仍=FAIL（117 pending + A06 环境红）——诚实覆盖缺口。

## 第七轮最终门（2026-10-24）

- **verify-r08-final-11**：补充叶 **PASS 11 / BLOCKED 115**（+设置会话置顶 0D4F0EFC9DAE〔投影断言补齐：pinnedAt/pinOrder 落列表+清除〕、+归档工作台会话 2D4EBE0FF3C2——full_original_behavior 双断言独立案例成立）；**A06 场景首次门内转绿**（a06 verdict=two-client-remote-pass__zero-touch-quit-verified failures=0——本轮 keystroke/CDP 路径成功；证明环境红为间歇而非永久，解锁手册继续有效）；a05 本轮间歇红（electron-exit 45s 超时——同族间歇签名，读腿 PPG-2 修复稳定，gate-09/10 两轮绿证在案）→ 场景 15/16（本轮 A05 红/A06 绿，与门 10 互为镜像）。
- **D-10 新发现登记**：旧栈会话内文本查找 `/api/sessions/find?path=&q=` 在 Rust 未迁移（rust find 仅按 id 定位且拒 q 键 400）——叶 4F1F69CCA760 不可绑定，post-R08 迁移项。
- **A06 宿主解锁手册交付**：`docs/rust-tauri/R08/A06_HOST_AUTOMATION_UNLOCK.md`（TCC 权限步骤/单套件验证/解锁后重跑预期/不弱化断言红线）。
- **七轮累计**：叶 2→**11**（D02 域 10 + D03 域 1）；7 真缺陷修复；伪差异澄清 3 起；语义差登记 D-7/D-8/D-9/D-10（各附根因）。
- overall 仍=FAIL（115 pending + a05/a06 间歇环境红〔两腿在门 09–11 各自轮换绿/红，解锁手册待宿主侧授权彻底消除〕）——诚实覆盖缺口。

## 第八轮最终门（2026-10-25）

- **verify-r08-final-12**：补充叶 **PASS 14 / BLOCKED 112**（+阅读会话消息页 AA9EB3FD034D、查看会话历史概览 CDBFE6616EB9、查看单会话摘要 F0319FC4F93D——读端口三叶 full_original_behavior：消息页旧栈客户端真实面 /api/sessions/messages vs Rust /lingxi/v1/sessions/{id}/history 统一历史读面双内核 200 对拍 + 概览/摘要已翻译读端口 + 无效会话 403|404 拒）；A06 本轮绿（连续第二轮）；a05 间歇红（electron-exit 同签名）→ 场景 15/16；命令 16/17。
- **八轮累计**：叶 2→**14**（D02 域 13 + D03 域 1，D02 66 叶中已证 13）；A05/A06 各有多轮绿证（间歇环境红，解锁手册在案）；7 真缺陷修复；伪差异澄清 3 起；语义差登记 D-7/D-8/D-9/D-10。
- overall 仍=FAIL（112 pending + a05/a06 间歇）——诚实覆盖缺口。

## 宿主解锁验证轮（2026-10-25，t16-*）

- **a06 转绿实证**：t16-a06-unlock-check-01 SUITE_EXIT=0（verdict=two-client-remote-pass__zero-touch-quit-verified failures=0）；门 11/12 已各有一轮门内绿。A06 环境红状态解除。
- **a05 精确诊断**（t16-a05-unlock-check-01..03 红链全保留）：TCC keystroke **已授权**（探针 exit 0）；剩余阻断=System Events 无法置开发版 Electron frontmost（`-2700`）→ Cmd+W/Cmd+Q 送错目标。线束对齐 a06 兜底纪律（row3 SIGTERM 机制入 verdict=Lead 接受同款；row1 AppleEvent 兜底后断言仍如实报 visible 不掩盖）→ a05 现 reach verdict=fail failures=1（唯 row1-hide，真实环境限制响亮呈现，不再 45s 超时崩溃）。
- **待用户动作**：重启终端宿主（TCC 不追溯已运行进程）后重跑 a05 单套件——键盘路径原生恢复预期即全绿。解锁手册已增补该节。

## 宿主重启后二次验证（2026-10-25，t17-a05-recheck-01）

- a05 重启后仍 `-2700`；**鉴别诊断定根因**：Finder frontmost/activate 同样静默无效 → 本 ZCode 进程上下文的 AppleScript 焦点变更被宿主策略整体丢弃（非 TCC/非 Electron/非线束）。a06 sigterm-graceful 机制与此完全一致（键盘路径同未达成）。
- 升级手段已入解锁手册：从普通终端 App 运行单套件（前台 App 上下文焦点变更放行概率高）；若仍红，row1 定性=宿主焦点策略环境受限（三不变量绿，仅 visibilityState 不可达），按 §6 如实 BLOCKED——当前 verdict=fail failures=1 即诚实形态，不改写为绿。

## Terminal.app 上下文第三级诊断（2026-10-25，/tmp/a05-recheck-terminal/）

- 经 `open -a Terminal` 真实普通终端上下文实测：System Events 报 -609/-1712（Terminal 未获自己的自动化授权）——三级诊断链闭合：ZCode=TCC 授权但焦点策略丢弃（-2700）；重启=无变化；Terminal=未授权（-609/-1712）。
- a05 全绿最后条件=为 Terminal/iTerm 授予同款两项权限并重启后重跑（手册三次更新节）。若不再尝试，row1 定性=宿主焦点策略环境受限（三不变量绿），verdict=fail failures=1 为诚实终态；a06 不受影响。
- **R08 最终态定格**：叶 14/126 PASS · 场景 15/16（A06 绿；A05=唯 row1 环境受限）· 命令 16/17 · 7 真缺陷修复 · D-7..D-10 语义差登记 · 交付物与解锁手册齐全。112 pending 叶继续补证与 PPG/D 系列产品裁决=待用户授权的 R09+ 域。

## Terminal 授权后第四级验证（2026-10-25 深夜）

- 用户为 Terminal 授予两项权限后两次实测（复用进程 + 完全退出后全新拉起，/tmp/a05-recheck-terminal 双记录）：仍 -1712/-609——Terminal→System Events 自动化在 TCC 层未放行。四级诊断穷尽（ZCode -2700 / 重启 / Terminal 复用 / Terminal 全新），仅剩用户目视系统设置勾选状态（会话内不可读 TCC 库）。
- **R08 终态裁定（本会话不再迭代 a05 环境）**：叶 14/126 PASS · 场景 15/16（A06 绿；A05=唯 row1 宿主自动化策略环境受限，四级诊断链+解锁手册完整在案）· 命令 16/17。a05 verdict=fail failures=1=§6 诚实终态。
- 后续（R09+ / 待授权）：112 pending 叶逐叶补证、D-7..D-10 与 PPG-1/3 产品裁决、a05 键盘路径在授权确认后的原生复跑。

## 第五级验证：目视确认 TCC 后全新 Terminal 仍 -1712/-609（终局）

- 用户目视确认 Terminal 两项 TCC 条目后，全新 Terminal 进程实测第三次重跑：System Events 仍 -1712/-609（21:33 记录）。**五级验证穷尽**：ZCode -2700 / 重启 / Terminal 复用 / Terminal 全新 ×2（授权前后各一）——Terminal→System Events 自动化在本机对 Terminal 客户端始终未放行（ZCode 客户端已放行但焦点策略丢弃）；不排除 MDM/配置描述文件层限制（会话内不可查）。
- **a05 row1 终局定性=宿主自动化策略环境受限**（三不变量绿，仅 visibilityState 不可达）；verdict=fail failures=1=§6 诚实终态。a06 不受影响。环境侧迭代到此为止——本会话对 a05 键盘路径不再有任何剩余手段。

## 终局第六查（2026-10-25）：描述文件清白 + TCC 库 SIP 拒读

- ManagedSettings 描述文件 payload 仅 allowSiri3489=0——排除自动化限制层；两级 TCC.db 直读均被 SIP 拒。六查穷尽（-2700/重启/复用/全新×2/描述文件/TCC 库），a05 row1=宿主自动化策略环境受限（§6 BLOCKED）终局定性不可再推进。
- **R08 会话就此关闭**：任务书 §8「完成本阶段后停止，不自动进入下一阶段」。两条剩余路径（宿主策略排查 / 112 叶 R09+ 补证授权）均为用户侧决策，交付物与证据链完整移交。

## 决策简报交付（2026-10-25 会话终）

- 终态审计核实无漂移（门 12 快照：14/15/16 + HEAD/分支/孤儿全净）。交付 `R08_DECISION_BRIEF.md`——两条解锁路径（① a05 宿主排查/异机重跑→17/17+16/16；② 112 叶 R09+ 补证显式授权）的对照与启动入口。
- R08 会话在会话内手段穷尽、§6 诚实终态、§8 停止纪律三重条件下正式收口；后续任一路径均由用户从简报入口启动。

## 第九轮最终门（2026-10-25）

- **verify-r08-final-13**：补充叶 **PASS 18 / BLOCKED 108**（+授权目录四叶 98DDC94776F1/604C64F4828A/1E0D477755CD/73B631D77531——读/增/撤/替换四操作双内核 200 对拍 + 无效目录双内核同 400 拒，full_original_behavior 机器核对成立）；A06 本轮绿；a05 间歇红（row1 环境受限终态不变）→ 场景 15/16；命令 16/17。
- **九轮累计**：叶 2→**18**（D02 域 17/66 + D03 域 1 + D04 域 1）；7 真缺陷修复；3 伪差异澄清；4 语义差登记。
- overall 仍=FAIL（108 pending + a05 环境受限）——诚实覆盖缺口；后续路径仍按 R08_DECISION_BRIEF.md 由用户决策。

## 第十轮最终门（2026-10-25，用户选路径①后）

- **verify-r08-final-14**（07:49→09:33）：补充叶 **PASS 20 / BLOCKED 106**（+记忆开关两叶 51905CECDD1B 查看会话记忆开关 / 9616596B5720 设置会话记忆开关——GET/PATCH /api/sessions/memory 已翻译端口双内核对拍 + 不存在会话 403|404 拒，full_original_behavior 机器核对成立）。
- **a05 首次以原生键盘路径 PASS**：verdict.json `hideMechanism=keyboard / quitMechanism=keyboard`，failures=0——不是回退。用户按路径①处理宿主授权后，本 ZCode 上下文的 System Events 焦点变更不再被丢弃；六级诊断链中的「宿主自动化策略环境受限」定性就此解除。a05 手动辅助模式（`R08_A05_MANUAL=1`）保留为备选，本轮未用、等待中的 Terminal 包装脚本已关闭以免与门跑争夺焦点。
- **a06 本轮 FAIL（exit 2）——键盘路径恢复后暴露的线束缺陷，非产品回归**：journal 顺序 `quit-b-keystroke-1(status 0) → electron-A-exit → quit-b-keystroke-2 → electron-B-exit`，即对 B 的 Cmd+Q 打进了 A；随后 A 侧 `client-a-still-remote` 的 CDP 求值等不到页面，`cdp-main-page: timeout after 90000ms` 崩溃。根因：System Events 的 `set frontmost of p` / `frontmost of p` 对**同 bundle 双 Electron 实例按 bundle 而非 unix id 回答**——以前键击被挡、走 sigterm-graceful 兜底时这一缺陷被盖住。修法（`r08_t03_a06_drill.mjs quitClientByPid`）：AXRaise+set frontmost 后改以 CDP 读目标渲染进程 `document.hasFocus()===true` 且其余存活客户端 `!==true` 才发 Cmd+Q；核实不到绝不盲发（盲发可退掉任意前台 App），直接走已披露的 SIGTERM 兜底；退 B 后新增硬断言 `client-a-alive-after-b`，A 被误伤时立即以明确错误终止而非 90s 超时。
- **t06_d3 本轮 FAIL（exit 1）→ 级联 A09/A10/A11/A12 四场景红**：rollback/reforward-B/A11/A12 全部 PASS 后，末尾替身 `/journal` 读取 `D3_ABORT TypeError: fetch failed`；门 11/12/13 该套件均 exit 0 → 偶发。abort 只打了 stack（无 cause、无替身退出状态），本轮根因**不可判定**，如实登记。加固（d1/d3/standin 同步）：abort 输出 `cause` 与替身 `{alive,exitCode,signal}` + stderr 尾；替身 `exit` 事件即时落日志；`/journal` 读取改 `node:http` `agent:false`（每次新连接，排除 undici keep-alive 复用服务端已关闭空闲套接字的竞态），替身进程仍存活时允许一次**有日志**的重试，已退出则直接响亮失败；替身自身 uncaughtException/unhandledRejection 落盘 `standin-crash.log` 后以 70 退出，`/journal` 坏行跳过并回 `malformedLines` 计数。
- **线束卫生**：t07 A/B 驱动正常收尾不回收桩供应商，每轮留一个 ppid=1 孤儿 node（本轮 pid 10678 已回收）；已补 `stub.kill` + `stub provider reaped` 断言。
- 本轮计：场景 11/16（A06 + A09–A12 红）· 命令 15/17 · 叶 20/126。十轮累计：叶 2→**20**（D02 域 19/66 + D03 域 1 + D04 域 1）；7 真缺陷修复；3 伪差异澄清；4 语义差登记。下一步：a06、d3 单套件经包装器取证后重跑最终门（目标 17/17 + 16/16 + 20 叶）。

## 第十一轮：t20 单套件取证 + verify-r08-final-15（2026-10-25）——命令/场景面首次全绿

- **t20-a06-focusfix-01**（修复后单套件，exit 0，failures=0）：journal 实证同 bundle 焦点歧义——`quit-b-raise` 后 CDP 读到 `{"target":false,"others":{"A":true}}`（两次），守卫拒绝盲发 Cmd+Q，B 走 sigterm-graceful、A 未被误伤（`client-a-alive-after-b` PASS）；随后 `quit-a-focus {"target":true}` → A 以真实键盘 Cmd+Q 退出。verdict=`two-client-remote-pass__zero-touch-quit-verified`，`quitMechanisms={B:sigterm-graceful, A:keyboard}` 如实披露。
- **t20-d3-standin-instr-01**（加固后单套件，exit 0，D3 DRILL OK，failures=0）：替身仅在演习末尾被自身 SIGTERM 回收（`STANDIN_EXITED code=null signal=SIGTERM`），无 FETCH_RETRY、无 CRASH——偶发未复现，取证武装到位（下次再红将有 cause + 替身退出状态 + standin-crash.log）。
- 桩回收逻辑另以独立探针实测：SIGTERM 后 202ms 优雅退出（exit 0）。
- **verify-r08-final-15**（10:27→12:08）：**命令 17/17 PASS · 场景 16/16 PASS · 叶 20 PASS / 106 BLOCKED**——R08 首次命令与场景面全绿。门内实证：a05 连续第二轮原生键盘（`hideMechanism=keyboard / quitMechanism=keyboard / manualMode=false`，failures=[]）；a06 `quit-b sigterm-graceful + client-a-alive-after-b PASS + quit-a keyboard`；d1/d3 全绿且无 FETCH_RETRY；t07_ab `PASS stub provider reaped :: exit=0`（无孤儿）。
- **overall 仍=FAIL，且只剩一个原因：106 枚 required_behavior_pending 叶按门规则记 BLOCKED（末阶段不得降级为 share-satisfied）**。这些叶需要 R09+ 授权（D01/D13/D20 新线束域 + D-7..D-10 / PPG-1 / PPG-3 产品裁决），即 R08_DECISION_BRIEF.md 路径②，超出本阶段任务书范围（§8 完成本阶段后停止）。
- **R08 会话内可达成面就此全部达成**：十轮叶 2→20 · 命令 17/17 · 场景 16/16 · a05 原生键盘恢复 · a06 同 bundle 缺陷定名并修复 · d3 偶发取证武装 · 7 真缺陷修复 · 3 伪差异澄清 · 4 语义差登记。工作区不 commit（任务书约束），全部改动与证据留在工作区待审。
