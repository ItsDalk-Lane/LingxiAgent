# EX02 / EX11 冻结交付

确认并修复两个真实缺口：压缩后技能正文/当前会话便签丢失；显式悬空分支头返回成功空历史，父环可能令导出不结束。已实现的旧纯文本/未知字段原档兼容保持原实现，不记为未实现。

- Base: `2ecd31181f6d8ac28a2fddb3d3cfbc4f214b83f7`
- 7 文件 candidate SHA256: `d462ed12af61f7c0b0391b3e9a86673cb673df3ca7eee10d9f08bb4bae1d7876`
- Patch SHA256: `c150b27490d0c9a539c0b722e5f15b7a00ef59d49c9ecfe4ca4e5058acedc295`
- 逐文件基线/候选：`ex02-ex11-recovery-manifest.json`；净补丁独立重放7文件逐SHA一致：`recovery-replay-result.json`。
- 原完整断言：`EX02-original-spec.json` / `EX11-original-spec.json`；逐原子条款：`ex02-ex11-atomic-evidence.json`。这些是交付资料，不修改正式SPEC/登记/门禁。

生产链：真实技能read成功记录与使用标记→压缩前恢复当前助手技能目录允许的正文/本SID便签→正常或修复摘要后附为普通历史→真实下一轮模型请求；当前目录以外路径只保留已读历史，不新增文件访问。读取沿现役Snapshot拒绝链接；6000/24000字符及16KiB UTF8限制有明确标记；未知旧recall和便签读取错误明确拒绝且原历史不变。源码调用仅lib的bind_recovery_sources最小hunk。

历史分页保留现役sequence兼容回退/重置边界；显式head缺失、负revision/seq、父序列不递减/重复、缺父明确报错；全分支遍历检查重复cursor/ID与revision，页上限200。旧纯文本继续运行；未知字段/opaque/raw仍可受控查看导出，不能安全进入模型的四类明确拒绝且零新增模型，DB原始内容逐字不变。

## 实际验证

- `recovery-service-frozen.log`：现役手动压缩13、自动压缩26、实际HTTP/WS历史9，合48 PASS。
- `recovery-kernel-final.log`：现役压缩规划44 PASS；`recovery-cms-final.log`：分页/分支9 PASS；`recovery-guards-frozen.log`：旧recall兼容/未知版本、UTF8/真实链接负控2 PASS。
- `recovery-experience-read-write.log`、`recovery-experience-feedback.log`、`recovery-experience-tools.log`：各1 PASS；经验开启/暂停/清空/非法输入/只读权限/反馈重复/跨助手/record recall真实工具链。
- 合106个实际测试 PASS。`recovery-clippy-frozen.log`：受影响service lib+三个父测试严格Clippy PASS。
- 真正回归前失败：`ex11-baseline-actual.log`（悬空head应500却200空）；`ex02-baseline-actual3.log`（实际read/notes后压缩缺正文）。前两个fixture准备失败日志保留，不能当生产失败。
- 最初完整回归私树缺少未修改的原报告前置（26中1失败）；补读base原文件 `docs/rust-tauri/R06/R06-T02_REPORT.md` 后全套重跑PASS，该报告不在净包、不改变任何断言。未重试碰运气。

真实外部副作用只在自建临时HOME/DB/本地受控HTTP Provider中；未读用户资料，未付费调用，未改Main，未新增依赖/target。

## 收据

- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-material-10556-1792040880900726000/ex02-real-provider-wire.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-material-18910-1792041041378633000/ex02-real-provider-wire.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-material-26273-1792041197928247000/ex02-real-provider-wire.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-material-37615-1792041476223128000/ex02-real-provider-wire.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-material-44883-1792041600787950000/ex02-real-provider-wire.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-notes-failure-18910-1792041041378632000/ex02-notes-failure-proof.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-notes-failure-26273-1792041197928244000/ex02-notes-failure-proof.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-notes-failure-37615-1792041476223099000/ex02-notes-failure-proof.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-notes-failure-44883-1792041600787874000/ex02-notes-failure-proof.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-pending-approval-26273-1792041197928243000/ex02-pending-proof.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-pending-approval-37615-1792041476223100000/ex02-pending-proof.json`
- `/private/tmp/lingxi-ex02-owned-evidence/lingxi-r06-ex01-ex02-pending-approval-44883-1792041600787878000/ex02-pending-proof.json`
- `/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-r06t04-svc-dangling-head-45045-1792041608312810000/ex11-corruption-proof.json`
- `/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-r06t04-svc-legacy-compatibility-45045-1792041608312868000/ex11-compatibility-proof.json`
- `/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-r06t04-svc-missing-parent-45045-1792041608962071000/ex11-corruption-proof.json`
- `/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-r06t04-svc-parent-cycle-45045-1792041608860242000/ex11-corruption-proof.json`

## 边界

本轮只验EX02/11当前核心要求，不冒称全部关联UI/Slash原叶重跑；保留原完整断言以便Root结合已验入口。Windows/Linux/完整打包 NOT_RUN。硬截断仍为现役明确HARD_TRUNCATE，不宣称恢复完整技能。原TS history-recovery对应的静默reasoning前缀修剪在Rust无该分支；当前不安全opaque明确失败保留原档，不制造虚假恢复。plan-file外层仍依原SPEC current_vs_later后续范围，不复活旧Agent loop。
