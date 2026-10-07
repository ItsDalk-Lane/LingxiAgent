import json,hashlib,os,datetime
from pathlib import Path
OUT=Path('/private/tmp/rr3-storage03-20261007')
META=Path('/private/tmp/rr3-g-review-02-20261007')
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
def save(n,x):(OUT/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
source=META/'owned-cache-boundary.json';raw=source.read_bytes();digest=hashlib.sha256(raw).hexdigest()
assert digest=='25dd2c48430e85d063037c33e11191bdd6f19450798d53168143a0ff0923febd'
body=json.loads(raw);assert 'Candidates are not deletion authorization.' in '\n'.join(body['notes'])
rows=json.loads((OUT/'objects.json').read_text());resolutions=[]
for r in rows:
 for hit in r['referenceHits']:
  p=Path(hit['path'])
  if p.name=='owned-cache-boundary.json' and hashlib.sha256(p.read_bytes()).hexdigest()==digest:
   hit['classification']='CACHE_INVENTORY_ONLY'
   resolutions.append({'object':r['path'],'reference':str(p),'sourceSha256':digest,'reason':'The document lists largestIntermediateCandidates and expressly separates actual runtime binary references. This is storage candidate inventory, not a requirement to preserve this failed object as an execution artifact. No historical document is changed.'})
 if r['decision']=='HOLD_REFERENCED' and all(x['classification']=='CACHE_INVENTORY_ONLY' for x in r['referenceHits']):r['decision']='PROPOSE_EXACT_UNLINK_AFTER_ROOT_AUTHORIZATION'
save('objects.json',rows)
save('reference-resolution.json',{'at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'resolutions':resolutions,'historicalDocumentsChanged':False})
safe=[x for x in rows if x['decision']=='PROPOSE_EXACT_UNLINK_AFTER_ROOT_AUTHORIZATION']
summary=json.loads((OUT/'summary.json').read_text());summary.update(proposedObjectCount=len(safe),proposedLogicalBytes=sum(x['bytes'] for x in safe),proposedStBlocksBytes=sum(x['stBlocksBytes'] for x in safe),heldObjectCount=len(rows)-len(safe),heldLogicalBytes=sum(x['bytes'] for x in rows if x not in safe),freeBytes=os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize)
save('summary.json',summary)
(OUT/'approved-candidates-pending-root.txt').write_text('\n'.join(x['path'] for x in safe)+'\n')
print(json.dumps({k:summary[k] for k in ['proposedObjectCount','proposedLogicalBytes','proposedStBlocksBytes','heldObjectCount','heldLogicalBytes','freeBytes']},indent=2))
