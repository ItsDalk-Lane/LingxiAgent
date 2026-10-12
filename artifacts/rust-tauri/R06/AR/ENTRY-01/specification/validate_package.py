#!/usr/bin/env python3
"""Validate this audit/prompt package only. Does NOT run project tests."""
from pathlib import Path
import hashlib, json, re, sys

def main():
    root=Path(__file__).resolve().parent
    issues=json.loads((root/'R06_问题与验收规格.json').read_text(encoding='utf-8'))
    checks=json.loads((root/'R06_逐项验收清单.json').read_text(encoding='utf-8'))
    prompt=(root/'R06_完整问题清单与修复验收总控提示词.md').read_text(encoding='utf-8')
    errors=[]
    def req(ok,msg):
        if not ok: errors.append(msg)
    req(issues['issue_count']==len(issues['issues'])==15, 'Issue count mismatch')
    req(checks['base_check_count']==len(checks['checks'])==60,'Check count mismatch')
    req(issues['audit_runtime_tests_executed'] is False, 'Incorrect claim of project test execution')
    ids=[]; cids=[]; flattened=[]
    for i in issues['issues']:
        ids.append(i['id'])
        req(i['status']=='OPEN', i['id']+': not an immutable open audit specification')
        req(i['runtime_reproduced_by_this_audit'] is False,i['id']+': fabricated runtime proof')
        req(len(i['acceptance_cases'])==4,i['id']+': missing per-item checks')
        req(i['id'] in prompt,i['id']+': not found in prompt')
        req(len(i['repair_steps'])>=4,i['id']+': insufficient repair steps')
        req(bool(i['sources']),i['id']+': missing source references')
        for s in i['sources']:
            req(issues['audited_sha'] in s['url'],i['id']+': source not pinned')
        for c in i['acceptance_cases']:
            cids.append(c['id']); flattened.append(c)
            req(c['issue_id']==i['id'], c['id']+': wrong parent')
            req(c['status']=='NOT_RUN' and c['evidence']==[], c['id']+': fake result')
            req(c['id'] in prompt, c['id']+': not found in prompt')
            req(all(c[k] for k in ('name','procedure','expected')), c['id']+': missing test details')
    req(len(set(ids))==15,'Duplicate issue IDs')
    req(len(set(cids))==60,'Duplicate check IDs')
    req(flattened==checks['checks'],'Checklist copies differ')
    req(prompt==(root/'R06_完整问题清单与修复验收总控提示词.txt').read_text(encoding='utf-8'),'TXT differs from MD')
    req('逐项自检' in prompt and '对抗性自检' in prompt and '独立对抗性审查' in prompt,'Missing required verification layer')
    req(prompt.count('```')%2==0,'Unbalanced code fences')
    req('�' not in prompt,'Replacement character in prompt')
    req('## 14. 最终交付与停止' in prompt,'Missing final delivery section')
    manifest=root/'MANIFEST.sha256'
    req(manifest.is_file(),'Missing checksum manifest')
    if manifest.is_file():
        names=set()
        for line in manifest.read_text(encoding='utf-8').splitlines():
            digest,name=line.split('  ',1); names.add(name); p=root/name
            req(p.is_file(),'Missing file '+name)
            if p.is_file():req(hashlib.sha256(p.read_bytes()).hexdigest()==digest,'Hash mismatch '+name)
        actual={p.name for p in root.iterdir() if p.is_file() and p.name!='MANIFEST.sha256'}
        req(names==actual,'Manifest coverage differs')
    if errors:
        print('DOCUMENT_SPEC_INVALID\n'+'\n'.join(errors),file=sys.stderr); return 1
    print(json.dumps({'result':'DOCUMENT_SPEC_VALID','issues':15,'checks':60,'software_tests_run':False,'all_checks':'NOT_RUN'},ensure_ascii=False,indent=2));return 0
if __name__=='__main__':raise SystemExit(main())
