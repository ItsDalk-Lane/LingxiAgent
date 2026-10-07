from pathlib import Path
import json,hashlib,datetime
R=Path('/Users/study_superior/Desktop/Code/LingxiAgent');O=Path('/private/tmp/rr3-doc-input-boundary-1pgmn8m7')
commands=[json.loads(x) for x in (O/'read-commands.jsonl').read_text().splitlines()]
rows={}
for c in commands:
 for f in c['files']:
  p=Path(f['path'])
  if not p.is_relative_to(R):continue
  rel=str(p.relative_to(R));rows.setdefault(rel,{'path':rel,'reads':[]})['reads'].append({'startUtc':c['startUtc'],'sha256':f['sha256'],'bytes':f['bytes'],'argv':c['argv']})
extra=['rust/crates/xtask/src/stage_maps/'+s+'.json' for s in ['R02','R03','R04','R05']]
extra += [x['path'] for x in json.loads((O/'document-inputs.json').read_text())]
for p in extra:rows.setdefault(p,{'path':p,'reads':[]})
for d in json.loads((O/'document-inputs.json').read_text()):
 rows[d['path']]['reads'].append({'startUtc':d['readUtc'],'sha256':d['sha256'],'bytes':d['bytes'],'argv':['inspect_inputs.py','full bytes; JSON keys or Markdown headings']})
for g in json.loads((O/'stage-command-graph.json').read_text()):
 rows[g['path']]['reads'].append({'startUtc':'see inspect_inputs.py command receipt','sha256':g['sha256'],'bytes':None,'argv':['inspect_inputs.py','full JSON stage map commands']})
for p,row in rows.items():
 data=(R/p).read_bytes();row['endSha256']=hashlib.sha256(data).hexdigest();row['endBytes']=len(data);row['changedAcrossRecordedReads']=len({x['sha256'] for x in row['reads']}|{row['endSha256']})>1
(O/'source-read-stability.json').write_text(json.dumps({'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'boundary':'仅本轮已读取文件；非全树冻结；未运行被测程序；读取后hash不冒称内核级原子读取跟踪','files':list(rows.values())},ensure_ascii=False,indent=2)+'\n')
summary={'recordedReadCommandsBeforeFinalizer':len(commands),'exits':{str(code):sum(x['exitCode']==code for x in commands) for code in sorted({x['exitCode'] for x in commands})},'sourceFileCount':len(rows),'filesChangedAcrossRecordedReads':[p for p,row in rows.items() if row['changedAcrossRecordedReads']],'outputScope':str(O),'testBuildHistoricalDriverExecutions':0,'repositoryWritesByThisAgent':0,'productVerdict':'NOT_ISSUED','R06_READY':False}
(O/'READ_QA.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(summary,ensure_ascii=False,indent=2))
