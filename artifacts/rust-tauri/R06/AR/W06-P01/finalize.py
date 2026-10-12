from pathlib import Path
import json,hashlib,time
root=Path(__file__).resolve().parent
owned=json.loads((root/'owned-files.json').read_text())
tags=['red-family-01','red-family-02','red-family-03','green-family-01','incumbent-oracle-01','final-four-c-01','final-regression-01','final-clippy-01','final-fmt-01','final-contracts-01','final-boundaries-01']
runs=[json.loads((root/f'{tag}.json').read_text()) for tag in tags]
final=[r for r in runs if r['tag'].startswith('final-')]
assert all(r['exit_code']==0 for r in final)
assert {r['tag']:r['test_count'] for r in final}['final-four-c-01']==13
assert {r['tag']:r['test_count'] for r in final}['final-regression-01']==77
samples=json.loads((root/'family-business-results.json').read_text())
assert all(r['matched'] for r in samples if r['run']=='final-four-c-01')
assert len([r for r in samples if r['run']=='final-four-c-01'])==19
C={
 'R06-AR-F06-C01': {'expected':'block/flow均不注入关闭的记忆；不支持键明确拒绝、zero provider wire','observed':'原10样本+新17异常主配置样本/普通control真实binary HTTP→Run→provider wire；普通false无记忆，异常400/failed/零请求','status':'SELF_CHECK_PASS','evidence':['final-four-c-01.json','family-business-results.json']},
 'R06-AR-F06-C02': {'expected':'引号内#、转义、中文、冒号、BOM/CRLF字符串保真','observed':'原具名gather/binary检查完整保留并实际通过，普通P01姓名保真；复合agent/name/yuan/locale键明确拒绝','status':'SELF_CHECK_PASS','evidence':['final-four-c-01.json']},
 'R06-AR-F06-C03': {'expected':'深层同名不串扰；语法/类型/重复敏感键错误明确拒绝，不默认开启','observed':'原普通错误/深层检查通过；adapter22键类型/anchor/merge/复合重复反例全部拒绝，实际patch拒绝且原字节不变；真正binary全族zero-wire和peer preview400','status':'SELF_CHECK_PASS','evidence':['final-four-c-01.json','family-matrix.json','family-business-results.json']},
 'R06-AR-F06-C04': {'expected':'缺/空/部分/现役例配置按原默认，未知兼容值更新保真','observed':'原缺空部分example亲跑绿；新增未知内部复合键/数值键及nested值patch语义相等；根/敏感分区非字符串键被明确拒绝，不冒称兼容归一化','status':'SELF_CHECK_PASS','evidence':['final-four-c-01.json']},
}
results={
 'work_id':'R06-AR-W06','finding_id':'R06-AR-F06','repair_round':'P01','agent':'/root/ar_w06_repair_01','status':'READY_FOR_INDEPENDENT_REVIEW',
 'independent_review':'NOT_RUN_NEW_R02_REQUIRED','base':'2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d','candidate_binding':'../W06-CANDIDATE-02/binding.json','frozen_owned':owned['files'],
 'fixed_self_check_cwd':'/Users/study_superior/.codex/worktrees/r06-w06-review2/LingxiAgent','target':'fixed cwd/rust/target;独立不共享',
 'service_binary_sha256':next(r['service_binary_sha256_after'] for r in final if r['tag']=='final-four-c-01'),
 'toolchain':{'cargo':'1.98.1 (797e8a9bc 2026-08-05)','rustc':'1.98.1 (48a229cea 2026-09-01)','cargo_absolute':'/Users/study_superior/.cargo/bin/cargo','compile_commands':'--locked -j8','fmt_exception':'cargo fmt为格式检查，无--locked/-j选项；完整实际命令保留','guard':'8192MB/600s，未关闭；受监督Python/Cargo父进程'},
 'C_ids':C,'finding_closure':[{'id':'R06-AR-W06-R01-F01','self_check':'CLOSED_IN_SELF_CHECK_NOT_INDEPENDENTLY_ACCEPTED','root_cause':'Value键类型与字符串lookup边界不一致，default/set之前遗漏非字符串敏感键','repair':'统一parse入口merge前检查全root/owned映射及全部merge来源、merge后再次检查；键不是String响亮拒绝；未知内部数据保留','evidence':['red-family-03.json','final-four-c-01.json','family-business-results.json','family-matrix.json']}],
 'runs':runs,
 'old_red_validation':{'adopted':'red-family-03','actual_tests':3,'passed':1,'failed':2,'normal_binary_control':True,'bad_owned_config_sha256':'e0dc11672cbcff4b8b4c03cd9a0fa2a54923aec0e8f4a5ff87b57a53eb0d34c8','real_business_failures':'17复合主配置/merge样本仍完成或默认，peer预览错误列共享技能；同断言产品修复后正确拒绝','excluded_attempts':{'red-family-01':'编译失败，无业务测试，不算旧红','red-family-02':'正对照断言误读flags层级；更正后以03亲跑确认，完整日志保留'},'historical_original_F06_old_scanner':'R01/E01旧flow/引号红已核对，未重新构建完整旧binary，不将其冒称本轮亲跑','intermediate_green':'green-family-01仅辅助；后来增加patch字节/READY断言使该测试摘要STALE，最终结果只用final-*固定副本'},
 'new_meaningful_tests':3,'old_tests_assertions':'test-preservation.json；所有原名称、断言、样本及原77数量保留','binary_family_business_cases':19,'adapter_invalid_key_samples':22,
 'original_scope':{'record_count':354,'r00_groups':101,'all_original_clauses_retained':True,'full_original_acceptance':False,'index':'original-obligation-review.json'},
 'remaining_scope':['新的独立Reviewer R02第三层亲验','F01/F05/F07/F08/EX等其它完整原义务与R06阶段门禁','R02—R05全阶段闭包/verify-R05未运行','其它OS/正式打包/真实供应商LIVE未执行'],
 'stale':['W06-E01/W06-R01候选验收不再适用于新agent_config/测试摘要','green-family-01后测试加强，仅辅助证据','后续修改任一owned产品/测试/配置须重新绑定受影响结果','F05缓存/F07目录与R02—R05消费者的阶段级集成应由F14/F15按最终组合候选重新执行；本项77影响回归不替代'],
 'permissions':{'git_write_commit_push':False,'R07':False,'real_data':False,'paid_provider':False,'real_external_send':False,'shared_skill_delete':False},
 'protected_vscode_settings_sha256':'0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3',
 'cleanup':'每binary样本SIGTERM成功wait；外部stub stop并join；仅删除自己合成临时fixture目录；每run wrapper新空home删除；shared技能在预览后断言存在，未调用删除业务；测试执行文件与binary证据见executable-binding.json',
 'timestamp':time.time(),
}
(root/'results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2))
report='''# R06-AR-W06 / F06 修复 P01

状态：**READY_FOR_INDEPENDENT_REVIEW**。一次性修复代理 `/root/ar_w06_repair_01`，未参与 E01 或 R01。没有自签独立 PASS；新的 R02 Reviewer 必须亲验第三层。

R01 HIGH 根因已在本轮自检关闭：根与敏感分区的非字符串 YAML 键不会再被当作缺字段恢复默认。本候选选择 R01 明确允许的“对不支持类型响亮拒绝”，没有声称将现役 js-yaml 复合键归一化为字符串。普通字符串键、block/flow、引号/BOM/CRLF、alias/merge 和原默认保持；未知字段内部数据保留。拒绝响应含 config.yaml 与 unsupported YAML key type，运行失败且零 provider 请求。

## 改动与根因全族

相对 E01，仅改统一 `agent_config.rs` 和三个专属测试；其余11 owned内容未变，没有加依赖、改Cargo、改消费者特例、写lib/shared/knowledge/session_tree、rootmeta或用户设置。

统一入口在读取默认值/更新之前，检查root和agent/memory/experience/skills/workspace_context的全部直接键；merge展开前递归检查所有来源（包括被显式覆盖的来源），展开后复核。字符串重复由真正YAML parser拒绝；字符串与复合键的规范化冲突在不支持键类型边界拒绝，无法借默认或set掩盖。

全族路径见 family-matrix.json：context_compiler主材料/roster；memory语言/flags/experience/update；memorytools执行时复核及skill启用；persona locale/yuan；skills读取/策略/名单/启停/删除引用/孤儿预览/runtime目录；agent_memory统一读写和原子patch；skill_library其它Agent引用。产品都继续复用同一入口，错误传播保留。

## 旧红到新绿及三层

1. 第一层逐项自检：最终固定副本四C 13/13，原影响回归77/77；四C逐条结果在 results.json。所有原测试名称/断言/数量保留，新增有意义测试3项：adapter全族拒绝与patch字节保护、未知内部值/merge保存保真、真正binary全族wire与shared预览。
2. 第二层对抗性自检：未修产品先亲跑 red-family-03，实际3测试、1 passed/2 failed；普通字符串配置真实binary对照正常，17主配置复合键/merge反例仍默认或完成、peer复合enabled预览错误列共享孤儿。采用同族断言修产品后全绿；最终固定副本19个业务样本全部符合期待，22个adapter键类型/alias/merge/复合重复样本拒绝，真实patch不改原字节。真实Gateway工具保留普通paused检查并新增3复合/merge拒绝，tenet不落盘。
3. 第三层独立审查：未执行，交新R02。13或77只是执行数量，不代替原语义完整验收。

所有失败尝试保留：red-family-01编译失败，不能算业务旧红；red-family-02测试正对照错误读了flags层级，修测试后采用03取得有效正对照与业务红。green-family-01随后增加原文件patch保全/READY断言，摘要STALE，仅辅助；最终采用固定副本 final-*。旧flow/引号基线红的R01/E01证据已核对，未冒称本轮重建完整旧binary。

## 固定候选与本人亲跑

HEAD/base 2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d。总控按15准确SHA复制绑定W06-CANDIDATE-02；本人最终只读 `/Users/study_superior/.codex/worktrees/r06-w06-review2/LingxiAgent`，使用该副本独立rust/target，不含W02/W12产品。开始/结束15SHA与主仓冻结一致，见 binding-before/after、owned-files.json。

绝对Cargo1.98.1/rustc1.98.1；编译与xtask均--locked -j8；fmt原生格式检查无--locked/-j参数，实际命令保留。guard8192MB/600s未关闭，有受监督父进程。每run新合成LINGXI_HOME；binary READY逐样本验证实际canonical home且source=cli。外部模型端点全是loopback替身，生产parser/gather/compiler/HTTP/Run/Gateway/数据库/组装均真实。

- final-four-c-01：exit0，13 passed。原10binary样本+新增18主样本及peer预览；gather/真实memorytool/保存字段。
- final-regression-01：exit0，77 passed。T01 service/kernel/closed_loop；T06 memory/skills/skill_context/hooks；adapter agent_memory。
- final-clippy-01：workspace/all-targets/-Dwarnings exit0。
- final-fmt-01：all --check exit0。
- final-contracts-01、final-boundaries-01：各exit0。

命令/start/end/exit/test_count/passed/failed/锁摘要/fixture摘要/15SHA/logSHA/binarySHA都在每run JSON及 results.json；非测试命令的test_count0不当作零匹配测试通过。实际test可执行文件摘要见 executable-binding.json。实际service binary SHA256：BIN_SHA。

## 原条款、未验与STALE

完整保留354原记录（10 Steps、6交付、4 A、263 R00断言、31相关修复Step、28 C、12旧review义务）及101叶组。original-obligation-review.json逐项追加本轮配置份额、证据、原owner/deadline及未验状态，历史R01评价单独保留；不拿本项关闭F05缓存、F07完整工具目录、F08或EX。

T01五Step：组成、artifact同源、静态前缀、中英文/子代理、冻结模板相关回归亲跑，配置份额已修；完整F01/F05/F07/EX份额仍OPEN。T06五Step：事实/compiled/经验/修订、隔离、技能审核/名单/策略、四hook、Gateway越界拒绝相关回归亲跑；完整记忆调度/生命周期、所有技能流程、worker/Pi与父子授权仍保留原到期义务。六交付物保留，不接受完整原Task。A01 wire/观测、A02 golden/segment/超限、A11隔离/工具绑定、A12 token/TTL/内容变更和越界/symlink两层仅接受配置影响自检份额，不扩大为全部工具wire预算或完整业务生命周期PASS。

未跑完整workspace tests、正式R06门禁、R05→R02阶段级闭包、其它OS、正式打包、真实供应商/LIVE。F14/F15须按最终组合候选重绑定与回归；本项独立77不替代阶段闭包。E01/R01旧候选及中间green-family测试摘要对本新候选STALE；后续产品/测试/配置修改使受影响结果重新STALE。

## 数据与边界

仅合成临时资料/loopback。真实cleanup只调用preview；普通共享引用预览空，复合peer明确400；预览后共享SKILL.md存在，未调用删除业务。每binary子进程SIGTERM成功wait、stub stop/join、仅自建临时fixture清理；wrapper新空home清理。历史W06-R01证据manifest逐条核验无变化。用户.vscode/settings.json仍是初始0f72…b3d3。无Git写、提交、推送、发布、真实资料、付费、外发或R07。

## 精确15 owned摘要

'''
report=report.replace('BIN_SHA',results['service_binary_sha256'])
report+='| 文件 | SHA256 |\n|---|---|\n'+'\n'.join(f"| {f['path']} | {f['sha256']} |" for f in owned['files'])+'\n'
(root/'REPORT.md').write_text(report)
manifest={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file() and p.name!='evidence-manifest.json'}
(root/'evidence-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2))
print(json.dumps({'status':results['status'],'final_runs':[{k:r[k] for k in ['tag','exit_code','test_count']} for r in final],'evidence_files':len(manifest),'service_binary_sha256':results['service_binary_sha256']},ensure_ascii=False))
