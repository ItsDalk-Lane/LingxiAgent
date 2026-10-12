import pathlib,json,hashlib,datetime

b=pathlib.Path(__file__).parent
r=b/'own.git'
a=r/'artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01'
def save(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
s=json.loads((a/'SCOPE_RECONCILIATION.json').read_text())
s['executor_status']='READY_FOR_INDEPENDENT_REVIEW'
s['final_selfcheck_window']='final-stable-direct-be-battery'
s['final_integrity_window']='final-freeze-direct-audit'
s['formal_gate_status']='NOT_RUN'
save(a/'SCOPE_RECONCILIATION.json',s)
records=[json.loads(x) for x in (b/'evidence/final-stable-direct-be-battery/evaluation.jsonl').read_text().splitlines()]
facts=[json.loads(x) for x in (b/'evidence/final-stable-direct-be-battery/raw/facts.jsonl').read_text().splitlines()]
be04=next(x for x in records if x['taskId']=='BE-04')
bound=[]
for leg in ['explainLeg','forceLegNormalPrompt','forceLegInjectionPrompt']:
    obs=be04['observedBehavior'][leg]
    row=next(x for x in be04['tokens']['ledgerRows'] if x['runId']==obs['runId'])
    accepted=[x['value'] for x in facts if x['kind']=='run_accepted' and x['value']['databaseHome']==row['databaseHome'] and x['value']['runId']==row['runId']]
    sql=[x['value'] for x in facts if x['kind']=='usage_sql_readback' and x['value']['databaseHome']==row['databaseHome'] and x['value']['runId']==row['runId']]
    endpoint=accepted[0]['providerEndpoint']
    exchanges=[x['value'] for x in facts if x['kind']=='provider_exchange' and x['value']['endpoint']==endpoint]
    bound.append(dict(leg=leg,run_observation=obs,accepted=accepted,full_production_sql=sql,provider_exchanges_same_endpoint=exchanges,ledger_row=row,physical_endpoint_binding='三条同服务请求的顺序/输入由 run_accepted 与 provider_exchange 原文共同核对；不能只用 endpoint 区分单次调用'))
save(a/'MEASUREMENT_BINDINGS.json',dict(window='final-stable-direct-be-battery',BE04=bound,aggregate_all_outcomes=be04['tokens'],semantics=dict(all_outcomes='三次调用真实测量合计100/39，其中失败任务仍消耗已测量tokens。',success_only='仅账本outcome=succeeded的一行40/17。另两行各30/11、failed合计60/22。',scores='四分是原限制/拒绝场景契约是否满足；不代表两项失败run变成成功任务。HTTP成功、schema合法、参数语义及实际任务终态分开。',unknown='NULL总量保持null并列覆盖率；部分缺测列已知小计；真实零须有响应与生产SQL绑定。'),coverage_controls=[x for x in facts if x['kind'] in ['wire_coverage_evaluation','coverage_controls','missing_measurement']],all_BE_scores=[dict(taskId=x['taskId'],score=x['score'],observedBehavior=x['observedBehavior'],tokens=x['tokens'],originalRequirementStatus=x['originalRequirementStatus']) for x in records]))
report=r/'docs/rust-tauri/R06/repair-current/W15_EXECUTOR_01_REPORT.md'
report.write_text('''# R06-AR-W15 / F15 Executor 01 冻结交接

状态：READY_FOR_INDEPENDENT_REVIEW。仅执行者局部自检；独立审查、正式 Stage 门禁 NOT_RUN，R06_NOT_ACCEPTED；未 commit/push。不执行 R07、不修改主目录或根控台账。

独立普通 clone：/private/tmp/lingxi-r06-ar-w15-e01-20261012T065826/own.git；真实 HEAD bc2ce51fd005ea75b9949156974e15d2761eb734。未继承主目录 dirty 产品或 .vscode。仅改原行为评测文件及新增证据核验脚本，生产代码未改。全部源码/只读输入清单由 frozen-source-manifest.json、frozen-inputs-manifest.json 绑定；完整原物留LOCAL_ONLY ownclone，SAFE仅携带两份实际候选源码、必要runner/报告和raw，不重复整套源码/原输入归档。不能只用 git diff 当有效输入清单。

旧正确断言先失败：BE04原报告常数0/0与真实100/39冲突；NULL缺测原混成0。旧失败与漏采样均保留。修复按生产账本全部明确列出的run统计，未知为null/覆盖率，真实零保留0；三腿BE04同窗loopback→realwire→SQL→evaluation合计100/39，成功账本40/17，失败账本各30/11。失败任务不等于零费用；收到有效HTTP/usage不等于任务成功。BE10仍为schema和工具选择合法、参数语义和任务效果失败，实际a.md未变、错误b.md被写，四维负控不删。

最后稳定13项自检通过；10项证据变异全部被拒绝，两项源码变异分别被旧正确断言拒绝。原BE01—BE10完整保留。BE06媒体、BE08脚本链无真实usage，明确NOT_MEASURED，未冒零。BE02完整catalog语义仍OPEN_F07，旧F07未冻结/缺源码，不继承10/10或13/13；只认F08已独立PASS自身份额。新增证据校验脚本只验证记录诚实，不替代完整业务验收。

适用回归：clippy、format、原ownership、API及协议生成契约检查通过。全workspace初次fail-fast窗口101/95.666秒和完整no-fail-fast窗口101/1042.879秒均保留。后者139个二进制摘要、1865通过、7失败、0忽略，6个原失败target源码与已提交HEAD逐字节一致。失败涉及取消队列、额外tool_result_model_projection事件、两项实际越界写入未拒绝；原断言未放宽，不判作环境豁免。registry实际7对期望9失败，F14保持OPEN。正式R05七顺序链及R06 Stage未跑；原2700/19800+有界开销预算没有压缩，不以workspace结果冒充正式门禁。

原8Tasks/37Steps/24交付/16A、1234义务/281叶/687断言、14旧Review/74条、90上游份额/12EX和28原R06 avatar/persona归属均逐项留在SCOPE_RECONCILIATION.json。原register SHA db4ceda76ca3911b9e4001934bd49f6d756c800634d3fe1da481730907876fae不变；所有未获完整业务验收项仍OPEN，不后移到期、不借R07豁免。独立production探针发现同session下一次wire仍读旧identity/AGENTS/memory；fresh session才生效。28份额部分route404/405、条件avatar GET仍200、URL直读正控200但服务URL stat/read400；mount未建立有效注册正控，保持缺口而非冒充完整mount验收。原义务登记不算修复。

证据质量：首次build的ps权限窗口INVALID；旧NULL红窗实际短寿命test PID未采到，RSS/时长null，不用cargo替代。首次final-be-battery并发JSONL有交错，INVALID_JSONL_FOR_ACCEPTANCE，已锁整行写后另开稳定窗口，未覆盖旧窗。旧监督comm字段截断把控制器当600秒业务限制，旧窗保留原较严记录；v2/v3按实际argv修正，业务仍600秒8192MiB，生产实际外部监督。OSHOME继承、LINGXI_HOME/TMPDIR独占synthetic、PRODUCT_DIR为own ROOT/lib、CARGO_INCREMENTAL=0、cargo --locked -j8。监测是采样，未知值null，不伪造OS最大RSS或精确短进程寿命。

完整workspace-continue窗口665个PID条目中有26种executables-after路径SHA为null（含截断僵尸名/系统PATH短命工具）；SAMPLING_AUDIT逐PID区分同PID另有service_ready绑定与确实缺路径测量，保留原null。不能宣称整窗全部实际程序前后绑定。final-stable-be-battery虽14PID无after-null，cargo重建造成test/service before-after SHA不同，原窗保留。最后另开final-stable-direct-be-battery直接运行已编好的实际test程序，测试/服务bin及源码前后SHA一致；只用该窗作为最终13项执行前后程序绑定，final-freeze-direct-audit核对该窗。库存/封存文件摘要不追认旧窗执行前测量。

交接证据在 artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01：evidence包含所有原失败/正式受影响回归/最后稳定窗口；executor-tools包含独立runner/probes/变异器；MEASUREMENT_BINDINGS保留三腿与覆盖率；PID_CLOSURE列全部采样PID及退出观察，漏采样单列；LOCAL_ONLY_INPUTS列synthetic配置、DB、二进制的有效输入SHA。实际用户数据、凭证文件、homes、DB、target、.git、node_modules不进入safe包；本地synthetic原物保留供必要只读取证。LIVE和现役引擎实跑NOT_RUN，固定loopback/脚本行为不冒真实供应商验收。
''')
print(json.dumps({'scope_status':s['executor_status'],'report':str(report),'binding_legs':len(bound)}))
