# 将本任务新回执压缩，按完整会话路径辨别通用文件名，避免同名不同原件误判。
from pathlib import Path
import json,gzip,hashlib,re,datetime,os
O=Path('artifacts/rust-tauri/R05/RR3/STORAGE-02'); T=Path('/Users/study_superior/.cache/lingxi-r05-neg-target'); I=T/'debug/incremental'
p=O/'all-before.json'; raw=p.read_bytes()
with gzip.open(O/'all-before.json.gz','wb',compresslevel=6) as f:f.write(raw)
assert gzip.open(O/'all-before.json.gz','rb').read()==raw
p.unlink(); allrows=json.loads(raw);del raw
refs=set(); hashes=set(); suffixes=set(); names=set()
def walk(v):
 if isinstance(v,dict):
  for k,x in v.items():walk(k);walk(x)
 elif isinstance(v,list):
  for x in v:walk(x)
 elif isinstance(v,str):
  refs.add(v); hashes.update(re.findall(r'\b[a-fA-F0-9]{64}\b',v.lower()))
  if '/' in v and len(v)<4096:
   name=v.rsplit('/',1)[-1]
   # 不通用的对象名仍采用历史文件名交叉保留。
   if name.endswith('.o'):names.add(name)
   if '/incremental/' in v:suffixes.add(v.split('/incremental/',1)[1])
for m in json.loads((O/'reference-scan.json').read_text())['files']:
 p=Path(m['path']);s=p.read_text()
 assert hashlib.sha256(s.encode()).hexdigest()==m['sha256']
 try:walk(json.loads(s))
 except ValueError:
  refs.update(s.split());hashes.update(re.findall(r'\b[a-fA-F0-9]{64}\b',s.lower()))
sessions={x['session']:x for x in json.loads((O/'sessions.json').read_text())}
records=json.loads((O/'owner-proof.json').read_text())['commands'];proof=[(datetime.datetime.fromisoformat(d['startUTC']).timestamp(),datetime.datetime.fromisoformat(d['endUTC']).timestamp(),d['receiptPath']) for d in records if d.get('startUTC') and d.get('endUTC')]
selected=[];protected=[]
for r in allrows:
 p=Path(r['path']); session=sessions.get(str(p.parent))
 if not session or not session['ownershipEstablished']:continue
 intervals=[x[2] for x in proof if x[0]<=r['birthTime']<=x[1] and x[0]<=r['mtimeNs']/1e9<=x[1]]
 if not intervals or r['mode']&0o111 or 'symlink' in r or not (p.suffix=='.o' or p.name in ['query-cache.bin','dep-graph.bin','work-products.bin']):continue
 matches=[]
 if str(p) in refs or str(p.relative_to(T)) in refs:matches.append('exact-path')
 if str(p.relative_to(I)) in suffixes:matches.append('full-session-relative-path')
 if p.name in refs or p.name in names:matches.append('literal-or-opaque-name')
 if r.get('sha256') in hashes:matches.append('content-sha256')
 r['referenceMatches']=matches;r['ownerSession']=str(p.parent);r['completedCommandReferences']=intervals
 r['ownershipBasis']='会话存在G唯一copy内嵌对象及同crate真实编译日志；本对象birth和mtime同时落在G已完成命令区间，排除旧hardlink复用；.o删除前还须MH_OBJECT'
 if matches:protected.append(r)
 else:selected.append(r)
(O/'candidates-initial-strict.json').write_bytes((O/'candidates.json').read_bytes())
(O/'candidates.json').write_text(json.dumps(selected,ensure_ascii=False,indent=2)+'\n')
(O/'protected-candidates.json').write_text(json.dumps(protected,ensure_ascii=False,indent=2)+'\n')
summary={'candidateFiles':len(selected),'candidateLogicalBytes':sum(r['bytes'] for r in selected),'uniqueInodeAllocatedBytes':sum(r['allocatedBytes'] for r in {r['inode']:r for r in selected}.values()),'protectedCandidateFiles':len(protected),'compressedOriginalReceipt':str(O/'all-before.json.gz'),'referenceRule':'路径/完整crate-session-file后缀/明确单独文件名/不通用.o对象文件名/内容SHA；其他会话中相同query-cache.bin等通用名不代表本原件'}
(O/'refined-selection.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n');print(json.dumps(summary,ensure_ascii=False))
