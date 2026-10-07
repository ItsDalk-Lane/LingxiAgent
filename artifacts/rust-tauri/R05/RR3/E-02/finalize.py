from capture import *
import copy,re
# 补齐当前消费者，报告与其摘要最后绑定，避免回填自造FINAL坐标。
def replace_key(text,key,value,indent=2):
 pattern=re.compile(r'^'+ ' '*indent +json.dumps(key)+r': ',re.M);matches=list(pattern.finditer(text));assert len(matches)==1,(key,len(matches))
 start=matches[0].end();_,length=json.JSONDecoder().raw_decode(text[start:]);rendered=json.dumps(value,ensure_ascii=False,indent=2).replace('\n','\n'+' '*indent)
 return text[:start]+rendered+text[start+length:]
g=ROOT/R/'G-REVIEW-01';obs={'utc':now(),'conclusionFiles':{n:(g/n).exists() for n in ['REVIEW.md','REPORT.md','RESULT.json','MANIFEST.json','manifest.json']},'boundary':'交付前真实复查；G尚无完整结论，不猜PASS/FAIL，不把部分case当16/16。'}
assert not any(obs['conclusionFiles'].values()),'G已经产生完成材料，先亲读全部原证再更新current'
for n in ['default16-01/summary.txt','default16-01/case-results.tsv','default-console.log']:
 p=g/n
 if p.is_file():obs[n]={'bytes':p.stat().st_size,'sha256':sha(p)}
dump(E/'g-observation-final.json',obs)
h=load(D/'R05_HANDOFF.json');current=copy.deepcopy(h['rr3_current']);current['historical_evidence_refs']['c']=current['evidence_refs']['c'];current['evidence_refs']['c']=current['evidence_refs']['cf46_review']
current['packages']['G']['note']='交付前已亲读最新矩阵及观察默认16运行：G-REVIEW-01仍RUNNING，无完整报告/manifest/结论；不猜PASS/FAIL或将部分case当16/16。'
for p in OWNED:
 if not p.endswith('.json'):continue
 text=(ROOT/p).read_text()
 if 'ORCHESTRATOR' in p:
  data=json.loads(text);stage=data['stages']['R05'];stage['rr3_current']=current;text=replace_key(text,'R05',stage,4)
 else:text=replace_key(text,'rr3_current',current)
 if p.endswith('R05_HANDOFF.json'):
  data=json.loads(text);unresolved=[{'id':k,**v} for k,v in current['packages'].items() if v['status']!='CLOSED'];text=replace_key(text,'unresolved_items',unresolved)
  additions=[R+'E-02/'+n for n in ['REPORT.md','raw-audit.json','source-audit.json','g-observation-final.json','d-object-readback.json','E01-manifest-consumption.json']];text=replace_key(text,'artifact_hashes',{**data['artifact_hashes'],**{q:sha(ROOT/q) for q in additions}})
 (ROOT/p).write_text(text)
dump(E/'current-state.json',current)
changed={p:{'beforeSha256':load(E/'before.json')['owned'][p],'afterSha256':sha(ROOT/p),'changed':load(E/'before.json')['owned'][p]!=sha(ROOT/p)} for p in OWNED}
dump(E/'changed-files.json',{'utc':now(),'changedCount':sum(x['changed'] for x in changed.values()),'ownedCount':14,'files':changed})
assert sum(x['changed'] for x in changed.values())==13
print('13/14 owned final current; G still running')
