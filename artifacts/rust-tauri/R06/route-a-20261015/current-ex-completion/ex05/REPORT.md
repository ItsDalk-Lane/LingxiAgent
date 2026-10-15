# EX05 恢复失败窗口修复

本包修复已独立重现的“第二段真实拒写后留下混合正文”，不代表阶段验收完成。Main、登记、门禁、原断言未修改，没有提交或推送。EX04 已另给当前手动核心独立17项通过；EX06助手嵌入选择真实缺口已交另一作者。

## 原失败与修复

原探针 `/private/tmp/r06-ex04-06-review/restore-second-write.rs`，原日志 `restore-second-write.log` 与事实 `restore-second-write-facts.json` 全保留。today.md 的真实 macOS uchg 使第二段写入 EPERM；旧实现 facts.md 和 memory.md 已变旧版本，另两段仍当前，且 pending不存在。强“失败后所有当前字节不变”断言真实 FAIL。原500绑定只触发目标修订读取失败，不能覆盖该写入窗口。

现在同助手操作门内，先保存全部受影响文件的原字节/缺失、计划结果及历史保留内容，再写正文。出错时只回滚仍能确认属于这次操作的字节；遇外部版本保留外部内容、保留待恢复日志，明确拒绝后续上下文读取。成功才剪裁到10份，始终保留选中目标与这次安全修订。失败回滚优先保全原有历史，可能暂多一份安全修订，不为凑10份删除用户旧历史；后续成功再正常剪裁。

恢复记录可从原真实详情接口 `lastRestore` 读回：running、succeeded、failed_rolled_back、failed_recovery_required、interrupted_rolled_back。进程退出后首次编译内容读取、上下文采集或恢复操作先处理遗留journal，未完成恢复不冒称成功。恢复过程中每份写前与最终发布前都核对版本，回滚不盲写无法识别的外部版本。真实目标日记只更新仍存在的日期，不重建已删来源；facts.db、原始来源、tenets、navigation不改。

真实HTTP成功后undo新增强原字节检查还发现旧实现固定英文标题；本包仅把真实助手locale传给DreamRevisionStore，保持当前中文编译内容格式。原字节断言未放宽。通用atomic_write仅补rename失败删除本次临时文件，防止拒写产生无用临时文件；没有改变写入权限、路径或其它存储合同。

## 净源与共享边界

3生产：agent_memory.rs（现有Dream journal/restore/prune及CompiledMemoryStore操作前恢复）、memory.rs（Dream getter locale/恢复与详情状态）、context_compiler.rs（实际采集memory.md前恢复）。3新增测试：adapter原独立失败修后测试、adapter完整故障/退出测试、service真实HTTP/上下文测试。无新依赖。

逐文件base/candidate/currentMain SHA见 source-sha.json。agent_memory/memory基线当时与Main完全相等。context_compiler私有基线早于Main已合实验proactive，净补丁只含gather旁一个独立hunk；Main原proactive内容不在本包diff，Root当前Main `git apply --check` 实际PASS。完整当前Main组合验证由Root执行；这里不会声称私树已包含所有未关联Main增量。

可信时序探针默认不存在，只能由宿主测试直接设置，HTTP和模型入参没有入口。真实进程在第一份物理写完成后exit77，随后独立新进程读取触发同一生产恢复；不是手填成功journal。没有把父进程称为OS文件沙盒。

## 实际验证

- 原第二写失败断言修后PASS，证据输出换新目录，原FAIL保持原样。
- 真实子进程退出与另一个新进程恢复；真实回滚再拒写后显式保留journal并在解锁后恢复；外部编辑不覆盖并阻止读混合内容；成功restore/undo与10份历史保留目标和安全修订，原始资料字节保全。adapter-final.log 含14旧 + 原反例1 + 4实质新场景与1子驱动（驱动无子环境时无业务断言，不虚算为新产品场景）。
- 真实HTTP恢复失败500/所有当前字节保全，真实403权限拒绝零更改，成功恢复/undo后ZH当前字节相同，另一助手不变，实际SQLite model_call_usage 0→0；实际 ContextMaterialSource 先拒绝外部冲突，再恢复后仅消费CURRENT内容。service-r2.log 新2/2通过。
- service-r2.log 附原19项：18通过，唯一手动skill安装原期望400实404发生于未同步Root EX03 GitHub安装增量的私树旧skill，不报Main回归、不改400；Root已接手用当前Main验证。与本包无关的旧源不进入净包。
- adapter-r2.log 的成功undo失败是夹具在抓before后才新加navigation，修正夹具先组装该真实来源再抓before，原字节断言不改。service-r1.log 的中文undo变英文是实际语言格式问题，按上文修源码；均保留。

最终R1冻结源码 Dream17（52.24秒）+新HTTP2（3.96秒）全PASS；R2未知operation收紧后 adapter14旧+6含子驱动全PASS，原restore3+详情1+列表1均PASS；lib/新测试严格检查、R2 adapter严格检查和fmt通过。完整机器收据 ../confirmation.json，最终源清单 ../FINAL-source-sha.json。R1的17+2未在仅收紧未知journal识别后重复，不据此冒称所有组合均重跑。只签本包当前macOS范围，正式Main合并与逐叶重绑由Root另实跑。

## 限制

只在当前macOS、合成数据、真实磁盘和本机供应商替身上运行。未执行Windows/Linux、Electron/Tauri窗口、掉电/磁盘硬件损坏或正式全阶段门禁；进程退出恢复不等于掉电事务。没有引入可隔离任意外部写者的OS文件锁或文件沙盒。已检测的外部版本明确拒绝并保留，无法辨识的冲突需要用户保留/整理后才能继续；不会自动抹掉外部编辑。

## R2最小增量

Root独立指出显式未知operation被误当旧v1。已实际构造 schema1/future-unknown 磁盘journal：原R1返回Ok(true)、当前正文被替换、journal删除，强“拒绝并保留字节” FAIL；原日志 unknown-operation-red.log。旧Rust创建点确实从未写operation。现在只允许无operation/schema1旧apply，或明确restore/schema1或2，其余显式null/未知operation/未知schema拒绝。相同反例修后Err且正文/journal原字节保留；unknown-operation-fixed.log。两文件净增量另存 ../r2-incremental，不能重新应用R1整包。

Root随后在当前Main也复现原手动skill缺包404/原期望400，并修复EX03 PreparedPackage NotFound分类；该修复不属于本包，本包绝不改400原断言。
