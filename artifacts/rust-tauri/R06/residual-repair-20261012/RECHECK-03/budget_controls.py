"""预算拒绝负控；可读真实BE记录，缺省仅使用明确标注的合成预算字段。"""
import argparse, copy, hashlib, importlib.util, json, sys
sys.dont_write_bytecode=True
from pathlib import Path

parser=argparse.ArgumentParser()
parser.add_argument('--records',type=Path)
args=parser.parse_args()
root=Path.cwd();audit_path=root/'scripts/rust-tauri/r06_evaluation_audit.py'
spec=importlib.util.spec_from_file_location('r06_evaluation_audit',audit_path)
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
if args.records:
    raw=args.records.read_bytes()
    selected={row['taskId']:row for row in [audit.parse_json(line) for line in raw.decode().splitlines()] if row['taskId'] in ('BE-02','BE-07')}
    assert set(selected)=={'BE-02','BE-07'},'缺真实BE02/BE07记录'
    input_kind='ACTUAL_BE_RECORDS';input_sha=hashlib.sha256(raw).hexdigest()
else:
    selected={key:{'modelCallsExpected':2,'modelCallsObserved':actual,'extraModelCalls':actual-2} for key,actual in [('BE-02',4),('BE-07',3)]}
    input_kind='SYNTHETIC_BUDGET_CONTROL_ONLY';input_sha=None
checks=[]
positive={key:{'modelCallsExpected':2,'modelCallsObserved':2,'extraModelCalls':0} for key in selected}
audit.enforce_call_budgets(positive)
checks.append({'control':'原预算内正对照','accepted':True,'inputKind':'SYNTHETIC_BUDGET_CONTROL_ONLY'})
for key,row in selected.items():
    assert row['modelCallsExpected']==2
    assert row['modelCallsObserved']==audit.FLOW_CALLS[key]
    assert row['extraModelCalls']==row['modelCallsObserved']-2
    try:audit.enforce_call_budgets({key:row})
    except audit.BudgetConflict as error:checks.append({'taskId':key,'control':'实际差额不能判原预算通过','rejected':True,'detail':str(error)})
    else:raise AssertionError(key+'超预算被当作通过')
    for mutation in ('hide_extra','inflate_budget'):
        changed=copy.deepcopy(row)
        if mutation=='hide_extra':changed['extraModelCalls']=0
        else:changed['modelCallsExpected']=changed['modelCallsObserved'];changed['extraModelCalls']=0
        try:audit.enforce_call_budgets({key:changed})
        except ValueError as error:checks.append({'taskId':key,'control':mutation,'rejected':True,'detail':str(error)})
        else:raise AssertionError(key+'预算变异被接受：'+mutation)
report={'inputKind':input_kind,'inputSHA256':input_sha,'auditSHA256':hashlib.sha256(audit_path.read_bytes()).hexdigest(),'controls':checks,'controlResult':'PASS','originalBudgetResult':'FAIL_BE02_BE07','formalAcceptanceComplete':False}
if args.records:assert args.records.read_bytes()==raw,'负控不能改写原记录'
output=Path(__file__).parent/('budget-controls-actual.json' if args.records else 'budget-controls-structural.json')
output.write_text(json.dumps(report,ensure_ascii=False,indent=2))
print(json.dumps({'inputKind':input_kind,'controlResult':'PASS','originalBudgetResult':'FAIL_BE02_BE07','checks':len(checks),'evidence':str(output)},ensure_ascii=False))
