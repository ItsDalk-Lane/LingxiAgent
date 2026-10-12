"""仅核对本轮两项真实凭证；复用当前核验器对应分支，不缩减完整BE电池入口。"""
import ast, hashlib, importlib.util, json, sys
from pathlib import Path
sys.dont_write_bytecode=True
root=Path.cwd();out=Path(__file__).resolve().parent
source_path=root/'scripts/rust-tauri/r06_evaluation_audit.py';source_bytes=source_path.read_bytes()
spec=importlib.util.spec_from_file_location('current_audit',source_path)
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
source_tree=ast.parse(source_bytes)
metrics=next(node for node in source_tree.body if isinstance(node,ast.FunctionDef) and node.name=='audit_metrics')
base=root/'artifacts/rust-tauri/R06/residual-repair-20261012/activation-validation'
names={'BE-02':'be02_on_demand_catalog_declared_per_turn_not_in_prompt','BE-07':'be07_material_citation_scope_isolated_and_locatable'}
report={'scope':list(names),'scopeResult':None,'fullAuditExecuted':False,'originalFullBatteryReduced':False,'formalAcceptanceComplete':False,'auditSHA256':hashlib.sha256(source_bytes).hexdigest(),'cases':[]}
for task_id,name in names.items():
    case_dir=base/('be02' if task_id=='BE-02' else 'be07');records_path=case_dir/'raw/t08-evaluations.jsonl';facts_path=case_dir/'raw/t08/facts.jsonl'
    records_bytes=records_path.read_bytes();facts_bytes=facts_path.read_bytes()
    records=[audit.parse_json(line) for line in records_bytes.decode().splitlines()]
    facts=[audit.parse_json(line) for line in facts_bytes.decode().splitlines()]
    record=audit.one(records,task_id+'实际记录');assert record['taskId']==task_id
    accepted=audit.one(audit.values(facts,'run_accepted'),task_id+'真实提交')
    launch=audit.one(audit.values(facts,'service_launch'),task_id+'启动')
    ready=audit.one(audit.values(facts,'service_ready'),task_id+'READY')
    stopped=audit.one(audit.values(facts,'service_exit'),task_id+'退出')
    final=audit.one(audit.values(facts,'world_final'),task_id+'最终文件')
    home=accepted['databaseHome'];run=accepted['runId']
    # 只允许当前明确合成目录及实际两份程序，不能借宽泛宿主根读取其它资料。
    allowed_dirs=[Path(home).resolve(),Path(final['workspace']).resolve()]
    for path in allowed_dirs:assert path.name.startswith('lingxi-r06t08-'+('be02' if task_id=='BE-02' else 'be07')+'-')
    allowed_files={Path(launch['binaryPath']).resolve(),Path(launch['testBinaryPath']).resolve()}
    expected_target=Path('/Users/study_superior/.codex/r06-w05-r01-runtime/target/debug').resolve()
    assert Path(launch['binaryPath']).resolve()==expected_target/'lingxi-service'
    assert Path(launch['testBinaryPath']).resolve().parent==expected_target/'deps'
    def exact_scoped_path(path,label):
        resolved=Path(path).resolve()
        audit.require(resolved in allowed_files or any(resolved.is_relative_to(parent) for parent in allowed_dirs),label+'超出本次两项独立合成资料/程序')
        return resolved
    audit.owned_path=exact_scoped_path
    assert launch['argv']==ready['argv'] and ready['pid']==stopped['pid']==accepted['servicePid']
    assert stopped['waitReaped'] is True and type(stopped['exitCode']) is int and stopped['exitCode']==0
    binary=exact_scoped_path(launch['binaryPath'],'实际程序');test=exact_scoped_path(launch['testBinaryPath'],'评测程序')
    assert launch['binarySha256Before']==stopped['binarySha256Before']==stopped['binarySha256After']==audit.sha_bytes(binary.read_bytes())
    assert launch['testBinarySha256']==audit.sha_bytes(test.read_bytes())
    a,full,wires,actual=audit.run_binding(facts,home,run,task_id)
    derived=[audit.ledger_view(a,row) for row in full]
    assert audit.same(record['tokens']['ledgerRows'],derived)
    audit.token_check(record['tokens'],derived,task_id)
    assert len(wires)==record['modelCallsObserved']==audit.FLOW_CALLS.get(task_id,audit.CALL_BUDGET[task_id])
    assert record['modelCallsExpected']==2 and record['extraModelCalls']==len(wires)-2
    assert record['originalRequirementStatus']==audit.SHARES.get(task_id,audit.LOCAL_SHARE)
    assert audit.same({key:record[key] for key in audit.DECLARATIONS[task_id]},audit.DECLARATIONS[task_id])
    assert audit.same(record['score'],{'toolSelection':True,'schemaValid':True,'semanticParamsCorrect':True,'taskComplete':True,'overall':'pass'})
    observation=record['observedBehavior']
    assert set(observation)==set(audit.OBSERVATION_KEYS[task_id])
    for key,value in audit.OBSERVATIONS[task_id].items():assert audit.same(observation[key],value),(task_id,key)
    assert actual['runs'][0]['status']=='completed' and observation['terminal']==actual['runs'][0]['terminal_reason']
    actuals=[actual];accepts=[accepted]
    workspace=exact_scoped_path(final['workspace'],'合成工作区')
    assert audit.same(final['workspaceListing'],audit.owned_listing(workspace,task_id+'最终文件'))
    before=audit.values(facts,'world_before_run');assert len(before)==1 and before[0]['input']==accepted['input']
    listing=dict(final['workspaceListing']);invocation=actual['invocations'];emitted=audit.calls(wires)
    # 执行正式核验器现行对应分支；完整audit_metrics入口仍要求全部十项和用量负控。
    branch=next(node for node in ast.walk(metrics) if isinstance(node,ast.If) and ast.unparse(node.test)=="task_id == '"+task_id+"'")
    scoped_code=compile(ast.Module(body=branch.body,type_ignores=[]),str(source_path),'exec')
    context={'task_id':task_id,'label':task_id,'facts':facts,'home':home,'wires':wires,'emitted':emitted,'listing':listing,'invocation':invocation,'observation':observation,'actuals':actuals,'accepts':accepts,'record':record}
    scope=dict(audit.__dict__);scope.update(context)
    exec(scoped_code,scope,scope)
    audit.enforce_call_budgets({task_id:record})
    controls=[]
    for name, updates in [('真实多一次', {'modelCallsObserved':3,'extraModelCalls':1}), ('改写期望次数', {'modelCallsExpected':3,'modelCallsObserved':3,'extraModelCalls':0}), ('隐瞒额外次数', {'modelCallsObserved':3,'extraModelCalls':0})]:
        bad=dict(record);bad.update(updates)
        try:audit.enforce_call_budgets({task_id:bad})
        except ValueError as error:controls.append({'name':name,'refused':True,'reason':str(error)})
        else:raise AssertionError(name+'被接受')
    assert records_path.read_bytes()==records_bytes and facts_path.read_bytes()==facts_bytes
    report['cases'].append({'taskId':task_id,'recordsSHA256':audit.sha_bytes(records_bytes),'factsSHA256':audit.sha_bytes(facts_bytes),'recordPath':str(records_path),'factsPath':str(facts_path),'businessAndEvidenceResult':'PASS_FOR_THIS_CASE_ONLY','physicalModelRequests':len(wires),'ledgerRows':len(full),'actualInvocationTargets':[row['target'] for row in invocation],'originalCallBudget':2,'extraModelCalls':record['extraModelCalls'],'originalBudgetResult':'PASS','negativeBudgetControls':controls})
assert source_path.read_bytes()==source_bytes
report['scopeResult']='TWO_CASE_RAW_EVIDENCE_AND_ORIGINAL_BUDGETS_PASS'
(out/'actual-evidence-check.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
print(json.dumps({'scopeResult':report['scopeResult'],'cases':len(report['cases']),'fullAuditExecuted':False,'evidence':str(out/'actual-evidence-check.json')},ensure_ascii=False))
