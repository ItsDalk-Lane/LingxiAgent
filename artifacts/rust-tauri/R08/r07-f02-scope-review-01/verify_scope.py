#!/usr/bin/env python3
"""独立重算 F02 范围；只核原断言、阶段与证据绑定，不执行或证明业务。"""
import collections
import hashlib
import json
from pathlib import Path
import sys
root = Path(sys.argv[1]).resolve()
read = lambda p: json.loads((root / p).read_text())
scope = read('docs/rust-tauri/R08/ADMISSION_F02_SCOPE_REVIEW.json')
bindings = read('docs/rust-tauri/R07/R07_LEAF_BINDINGS.json')['leaves']
original = {l['id']: l for l in read('docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json')['supplemental_scenarios']}
full = {k:v for k,v in bindings.items() if v['basis'] == 'full_original_behavior'}
errors = []
for leaf in scope['leaves']:
    key=leaf['id']; bound=full.get(key); old=original.get(key)
    if not bound or not old:
        errors.append([key,'missing original/binding']); continue
    for actual, expected, label in [
        (leaf['executionStageIds'],old['execution_stage_ids'],'stage'),
        (leaf['executionTaskIds'],old['execution_task_ids'],'task'),
        ([a['originalAssertion'] for a in leaf['assertions']],old['assertions'],'original assertions'),
        ([a['currentCases'] for a in leaf['assertions']],bound['assertionCases'],'case bindings'),
    ]:
        if actual != expected: errors.append([key,label])
if set(full) != {l['id'] for l in scope['leaves']}: errors.append(['all','full original inventory'])
ui = {k for k,v in full.items() if 'r07_ui_vitest' in v['suiteRefs']}
a23 = {k for k,v in full.items() if any('r07_t12_closure::r07_t12_a23_runtime_route_matrix' in g for g in v['assertionCases'])}
later = {k for k in full if any(s > 'R07' for s in original[k]['execution_stage_ids'])}
for actual,key in [(ui,'uiVitestLeaves'),(a23,'a23AssertionSubstitutionLeaves'),(later,'alreadyMultiStageFullOriginalLeaves')]:
    if actual != set(scope['groups'][key]):errors.append(['all',key])
sources={}
for field in ['sourceBindings','uiTestSourceBindings','candidateProofSourceBindings']:
    for path,expected in scope[field].items():
        digest=hashlib.sha256((root/path).read_bytes()).hexdigest();sources[path]=digest
        if digest != expected:errors.append([path,'source drift'])
union=set().union(*(set(v) for v in scope['groups'].values()))
counts={'registeredLeaves':len(bindings),'fullOriginal':len(full),'existingStageShare':len(bindings)-len(full),'withUiVitest':len(ui),'pureUiVitest':sum(v['suiteRefs']==['r07_ui_vitest'] for v in full.values()),'usesA23AsAssertionProof':len(a23),'a23BoundAssertions':sum('r07_t12_closure::r07_t12_a23_runtime_route_matrix' in g for v in full.values() for g in v['assertionCases']),'sameCauseScreeningUnion':len(union),'otherFullOriginalNotSemanticallyCertifiedHere':len(full)-len(union),'eligibleFullOriginalWithLaterStages':len(later)}
for k,v in counts.items():
    if k in scope['counts'] and scope['counts'][k]!=v:errors.append([k,'count'])
result={'schema':'lingxi.r08.independent-f02-scope-verification.v1','status':'PASS_SOURCE_RECONSTRUCTION_ONLY' if not errors else 'FAIL','businessAcceptance':'NOT_EXECUTED','reviewer':'admission_evidence_review','counts':counts,'originalsAndBindingsUnchanged':not errors,'sourceBindings':sources,'scopeJsonSha256':hashlib.sha256((root/'docs/rust-tauri/R08/ADMISSION_F02_SCOPE_REVIEW.json').read_bytes()).hexdigest(),'errors':errors}
print(json.dumps(result,ensure_ascii=False,indent=2))
sys.exit(bool(errors))
