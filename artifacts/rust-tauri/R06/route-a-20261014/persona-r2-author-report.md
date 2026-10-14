# 人格来源竞态 R2 修复增量交付

工作树：/private/tmp/lingxi-r06-persona-completion。没有改主工作树，未提交/推送/改门禁或状态。

增量基线：主控已合并的 /private/tmp/r06-persona-completion.patch，SHA256 9040141da4d7f2e1a16603637779259cefdc92917fd5bdc63efe020597864de9。修复前 persona.rs SHA为 ba0e79d58c8e0cce643c2d197f3e891669c2b3e99bb7947213f632797af682a3，test SHA为 0b3725d30a9c8bd7f185fb0fc73573f32a451782efd6e46518c8524c6883607d，两者与主树读取值一致。本次只修改这两个文件。

本增量：/private/tmp/r06-persona-r2-incremental.patch
SHA256：3d77b4c24b6a8dfded32a84b7bf260856d9a1c0676f226357952a5ece518de9f
禁止把此前全量人格补丁再次应用；本文件仅相对上面已合并版本的差异。

## 事实与实现

已实际复现独立Reviewer反例：正常服务/认证HTTP/真实模型网关请求已经发出，网络替身暂不回响应；测试进程作为外部writer改AGENTS，再放行模型。旧源返回200，RED日志 /private/tmp/r06-persona-r2-red.log，行为与Reviewer日志一致。

修复使 description_for 从同一文件字节快照解析双方人格、config.yaml、preferences.json与实际被消费的模板/最终样例；回落链上尚缺失的优先候选和缺失的自有正文也记录，所以文件新出现改变优先级仍409。不是先记录一次、生成时再另读一遍导致材料错配；配置读取复用既有字节预算。description.md本身也参与提交前检查，避免覆盖并发更改。源快照在生成前检查一次，在模型用量结算后、任何正文/简介写入前再次检查。

提交检查不通过时明确409/persona_changed，服务不再改动目标正文、原简介及事件，外部后改完整保留。已完成供应商响应仍真实入账为Succeeded（usage已报告；本地提交冲突不会抹掉或改写模型调用事实），未借冲突重发模型。正常重试采集新快照后200并正常发布。简介缓存sourceHash另外包含实际语言，避免相同源正文但输出语言变化时复用旧简介。此次未修改头像、网络配置策略、供应商/凭证失败语义或其它代理模块。

## 新增真实证明

保留原11项，增加一项13变体竞态矩阵 description_all_material_races_refuse_stale_commit_and_preserve_paid_usage。

13变体：另一AGENTS、另一identity、本次目标identity；Agent名称、yuan选择、Agent语言；用户名称、全局语言；实际生效模板、最终样例；原先缺失的优先语言模板、新出现的自有正文；写空目标时实际采用的identity模板。

每变体有：模型收到生产请求的确定性窗口→线体明确包含原材料→外部真实落盘修改→模型真实回应→409、原目标与简介/事件不变、外部修改保留、已完成模型账本1行/Reported usage/transport_attempts=1/Succeeded→不再修改输入的正常重试200且简介更新/事件+1。fixture只门控外部供应商HTTP回复，不mock PersonaService/源读取/网关/存储/生产检查。

## 验证

指定 /Users/study_superior/.cargo/bin/cargo 1.98.1 + --locked，合成HOME/TMP/LINGXI_HOME/产品根与仓库外target沿用前批；未读取真实业务数据。

- 竞态矩阵13种拒绝+13种正常对照全通过，单项32.74s，/private/tmp/r06-persona-r2-green-races.log。
- 原11+新增矩阵：12/12 PASS，90.89s，/private/tmp/r06-persona-r2-suite.log。
- 改动crate+newtest严格clippy --no-deps -D warnings通过，/private/tmp/r06-persona-r2-clippy.log。
- fmt --all --check、git diff --check通过，/private/tmp/r06-persona-r2-fmt.log。
- 已在主控当前工作树只读执行 git apply --check 增量，干净通过；没有修改主树。
- 未重复其它25个既有回归、未运行整阶段门禁、未自称独立Reviewer PASS，需主控合并后交同一Reviewer重验。
