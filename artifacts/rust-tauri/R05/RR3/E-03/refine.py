import json, pathlib, re
from audit import ROOT, EV, DOCS, atom, load, save

def field(raw,key,value):
    m=re.search(r'^  '+re.escape(json.dumps(key))+r': ',raw,re.M)
    value=json.dumps(value,ensure_ascii=False,indent=2).replace('\n','\n  ')
    if m:
        _,n=json.JSONDecoder().raw_decode(raw[m.end():]);return raw[:m.end()]+value+raw[m.end()+n:]
    p=raw.rfind('\n}');return raw[:p]+',\n  '+json.dumps(key)+': '+value+raw[p:]

banner='> **RR3 E-03生成截点（2026-10-07）：NOT_ACCEPTED / R06_READY=false；offline_gate=BLOCKED。** G02空间阻断，默认16/full R02/full E5及新FINAL未完成；最近正式阶段仍RR2/FINAL历史FAIL。A/B/C-F46/H/I/J包级CLOSED；D历史LAN阻断待未来确切对象复核；原E两mustFix已独立关闭，本E03仅SELF_CHECKED待全新审。后续独立E结论与真实Git回执另行消费，不预写未来PASS。现行范围见[R05_REPORT §12](R05_REPORT.md#rr3-current)，此前各轮原文均保留为历史。'
for name in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md']:
    p=ROOT/'docs/rust-tauri/R05'/name;s=p.read_text();s,n=re.subn(r'^> \*\*RR3 当前（[^\n]*',banner,s,count=1,flags=re.M);assert n==1
    if name=='R05_REPORT.md':s=s.replace('真正当前阶段状态以§11及HANDOFF rr3_current为准。','E-02当时当前指针为§11；本轮生成截点以§12及HANDOFF rr3_current为准。',1)
    atom(p,s.encode())

c=load(EV/'current-state.json')
c['packages']['E']['snapshot_semantics']='SELF_CHECKED/PENDING仅表示E03生成截点。后续全新E独立报告与总控最新协调记录、真实交付回执为新结论来源；未存在的报告不填路径/哈希/PASS，不要求本报告循环绑定自身提交。'
for path in [p for p in DOCS if p.endswith('.json') and not p.endswith('ORCHESTRATOR_PROGRESS.json')]:
    raw=(ROOT/path).read_text();raw=field(raw,'rr3_current',c)
    if path.endswith('R05_HANDOFF.json'):
        h=json.loads(raw);items=h['unresolved_items'];items=[dict(id='E',**c['packages']['E']) if x['id']=='E' else x for x in items];raw=field(raw,'unresolved_items',items)
        raw=field(raw,'subsequent_receipt_consumption',dict(generated_snapshot='E-03 SELF_CHECKED; independent result not yet issued at this cutoff',independent_review_receipt_ref=None,coordination_refs=['docs/rust-tauri/R05/repair-current/RR3_ISSUE_MATRIX.json','docs/rust-tauri/R05/repair-current/RR3_PROGRESS.md','docs/rust-tauri/R05/repair-current/RR3_HANDOFF.md'],rule='后续消费者先读真实最新独立E报告/总控协调回执与Git独立收据，按实际UTC/摘要判新结论；本E03不提前签未来PASS，也不要求更新文档自身SHA来证明自身。G/FINAL未完成始终不得据E PASS放行。'))
    atom(ROOT/path,raw.encode())
path=ROOT/'docs/rust-tauri/ORCHESTRATOR_PROGRESS.json';raw=path.read_text();d=json.loads(raw);r=d['stages']['R05'];before=json.dumps(r,ensure_ascii=False,indent=2).replace('\n','\n    ');r['rr3_current']=c;after=json.dumps(r,ensure_ascii=False,indent=2).replace('\n','\n    ');assert before in raw;atom(path,raw.replace(before,after,1).encode())
save('current-state.json',c)
print('current banners and future receipt consumption corrected; historical bodies retained')
