# R00-T01 第 4 轮独立对抗性验收

**VERDICT: PASS。** R00-A01、R00-A02 两项 REQUIRED 均有与当前候选一致的真实 PASS 证据，未发现 BLOCKING finding。本结论仅放行 R00-T01；R00 其他任务、图形桌面、安装包和其他平台不在本次验收范围。审查对象是分支 `codex/rust-tauri-migration`、HEAD/Task Base SHA `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d` 的未提交工作树。未提交或推送。

## 规格、源码与交付核对

从头读取任务书总入口、01—06 共同约束、R00 阶段、90 来源、91 交付规则，以及 JSON 目录中 R00-T01、R00-A01/A02；另读当前 `AGENTS.md`、`CONTRIBUTING.md`、`tests/README.md`、现行 BASELINE/DELTA/Task 报告、R1/R2/R3 审查与 R2/R3 根因矩阵。以当前源码和本轮复测判定，未继承先前 PASS。核对 `BASELINE.json` 登记的 16 个规则、源码、测试文件哈希，全部仍相符；Node v24.16.0、npm 11.13.0、macOS 27.0 arm64、当前分支与 `origin/HEAD → origin/main` 也与记录一致。

五个 Step 均已核对：实际规则已记录；研究 SHA 等于实施 HEAD，故无已提交源码差异及待重定位路径；首次工作树的 17 个旧任务书删除、33 个新任务书未跟踪文件明确区别于 T01 产物；已建立隔离 `HOME`、`TMPDIR`、`LINGXI_HOME`，并实际确认 `scripts/launch.js → scripts/dev-env.js` 会覆盖单独传入的 `LINGXI_HOME`；实施保持在原分支和可恢复的 HEAD，没有 reset、提交或远程操作。三项交付 `BASELINE.json`、`BASELINE_DELTA.md`、隔离目录真实路径与启动探针均在，当前场景结果由 `R00-A01.result.json`、`R00-A02.result.json` 记录。

真实开发服务链由 `desktop/main.cjs` 派生 `server/bootstrap.ts`，再进入 `server/main-full.ts → server/index.ts:startServer → ensureFirstRun/LingxiEngine.init`。探针运行同一服务组合入口，未 mock 被验收服务；这不等于桌面窗口或安装包验收。生产源码、配置及三组相关测试未被 T01 候选改动。

## 独立实测

| 项目 | 本轮结果 |
|---|---|
| 安静窗口 | 探针前后 `ps`、两个真实目录的 `lsof +D` 均未见 Lingxi.app、相关 server 或打开句柄；探针自身 `quiet_before` / `quiet_after` 均为 true。用户授权暂时关闭应用，本轮没有重启它。 |
| R00-A01 真实服务 | 在 `/tmp/r00-review-r4-probe.tKHMBz` 另存本轮原始输出；`startup-probe-r3-113a24e0f674d69d5a6611e8.json` 的 SHA-256 为 `1a5699393c1c30b88e3643bc66e6a38747018ed6ba659f4b57a367660dbaa4ee`。探针退出码 0，健康检查 200/ok，服务退出码 0，`server-info.json` 关闭后消失。服务参数和日志中的实际 home 指向 `/tmp/lingxi-r00-a01-r2-g8u4jd6y/isolated/lingxi-home`，该隔离目录产生实际数据及日志。 |
| 两个真实目录 | `~/.lingxi`、`~/.lingxi-dev` 内各自排他创建的 `0400` 哨兵，身份和内容前后相同；两目录的完整摘要、内容摘要均前后相同，扫描错误为 0。macOS 沙盒对两处拒写均返回 EPERM；哨兵及本次隔离子目录最终均消失。原始结果的 14 项 acceptance check 全为 true，`errors`、`creation_failures` 均为空。脚本摘要 `4079f323b787912e67fb3baca0c59426a1d3ef2dd3ec1c672823e04255da303a` 与当前文件一致。 |
| 清理对抗回归 | 在 `/tmp` 复跑 `test_startup_probe_r2` 与 `test_startup_probe_r4`：22 项通过，退出码 0。覆盖首/第二目录的最终父目录关闭前后报错、普通文件/链接在原名最后检查后替换、外来项恢复或保留、目标预占、移动后报错、异常创建共用清理、隔离目录换名与收尾失败。另在本机真实调用 `renameatx_np + RENAME_EXCL` 的能力探测：目标已占时返回 EEXIST 且两项身份不变，同一目录空目标移动成功，清理后目录为空；真实 A01 创建哨兵前也在各自同卷隔离目录执行此探测。 |
| R00-A02 与候选负向 | 在 `/tmp` 复跑工作树和候选负向测试各 1 项，均通过。`verify_candidate_r2.py` 自己现场读取当前 HEAD/分支、17 个删除及二进制 diff、33 个原有未跟踪文件逐项哈希与集合、暂存区及其他已跟踪差异，前后两次采样一致；其输出为 `ok: true`、92 项、退出码 0。独立调用 `worktree_current.collect/verify_first_snapshot` 的九项检查也全为 true。 |
| 服务相关回归 | 使用另一组 `/tmp` 隔离环境执行 `npx vitest run tests/startup-contract.test.ts tests/hana-runtime-paths.test.ts tests/server-composition-boundary.test.ts`：3 文件、26 项通过，退出码 0。内含从真实 `server/bootstrap.ts` 启动完整服务组合并经 HTTP 验证认证、健康和关闭的链路。原始输出 `/tmp/r00-review-r4-vitest.txt`，SHA-256 `45a76acef977f9332ed5db89d3779575fee3dec855fc0fa445faf1bb3808551a`；故障回归输出 `/tmp/r00-review-r4-regression.txt`，SHA-256 `0ff4be7dab36d9a9da0691e0e3099636600f9c72146188925c25b8d5cd079023`。 |

## 历史 finding 逐项结论与证据一致性

- **R1 F01 / R2 F01：**真实目录不再由受控假目录或仅元数据代替；本轮实际设置真实只读哨兵，逐文件内容及完整摘要稳定，前后安静窗口均成立。R2 首次目录完整摘要变化的退出码 1 仍保留，未追认 PASS。
- **R2 F02 / R3 F01：**创建事务在排他创建前登记；文件及最终父目录关闭位于统一异常清理范围。首/第二目录、关闭实际生效前/后报错的 `/tmp` 反例均未遗留自有哨兵，异常仍记失败。
- **R3 F02：**正常和创建失败清理调用同一 `delete_owned_sentinel`。源目录名字经 `renameatx_np + RENAME_EXCL` 排他移入本次 `0700` 同卷子目录，移入后核对对象身份、类型、权限及内容；普通文件/链接替换不会被当作自有哨兵删除，源名字空时排他恢复，否则保留可找回位置并失败。目标预占不覆盖，`EXDEV` / 不支持不降级为普通改名或原路径删除。
- **R2 F03 / R3 F03：**候选动态枚举和总校验器当前工作树复核均已实测。审查报告写入前两个候选目录共 94 个普通文件；候选明确只排除自引用的 `candidate-summary-r2.json` 和校验输出 `candidate-verify-r4.json`，其余 92 个纳入。按总控提供的“相对路径 + NUL + 文件内容哈希原始 32 字节”规则独立重算全 94 项，聚合 SHA-256 精确为 `21fd76c1201456207fb88222b2dd166da799253e4699d3778f32eaa61f3fa6f7`；按候选自身排序规则重算 92 项聚合值为 `1806a4932675d9d04a27f6bb418d538f1043ec8b249f71d6a18af2bb92c8e145`，与声明一致。A01/A02 的 64 条证据引用哈希全部匹配。最终探针版本与当前文件一致；R1 三次退出码 `0,1,0` 和后续八次 `1,0,1,0,0,0,0,0` 原始尝试仍可查，失败未被删除或改写。

**REQUIRED 结论：R00-A01 PASS，R00-A02 PASS。Findings：无。** 同 UID 恶意进程可在隔离目录内于最终核对与删除之间再替换对象的剩余风险，候选已有 `/tmp` 反例和明确说明；本轮按 R00-T01 原规格的已确认安静窗口作判断，没有把其描述成已消除，也没有据此扩大原验收条件。检查交付目录的文本/JSON 日志，未见真实用户文件内容或密钥被记录；原始输出使用摘要和必要进程/目录信息。独立复测原始输出仅在 `/tmp`，未写现行 Task 结果。本报告是冻结候选后的唯一新增文件，故其自身不在上述 94/92 项摘要中；写入后的动态候选集合会增加一项，不能把写入前的 `ok: true` 称为包含本报告的校验。
