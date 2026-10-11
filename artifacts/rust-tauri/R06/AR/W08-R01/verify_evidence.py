import json,hashlib,sqlite3
from pathlib import Path
OUT=Path(__file__).resolve().parent
summaries=[]
def canonical(x):return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def render(r):
 if r['status']=='failed':return 'tool error ['+r['error']['code']+']: '+r['error']['message']
 if r['status']=='cancelled':return 'tool call cancelled by the host before completion'
 parts=[]
 for b in r['content']:
  if b['type']=='text':text=b['text']
  else:
   b=json.loads(json.dumps(b));resource=b.get('resource',{})
   if resource.get('uri','').startswith('file://'):resource.pop('uri')
   text=canonical(b).decode()
  if text:parts.append(text)
 for ref in r.get('resourceRefs',[]):
  name=ref.get('displayName',ref['resourceId']);uri=ref.get('uri','');parts.append('[resource: '+name+(' <'+uri+'>' if uri and not uri.startswith('file://') else '')+']')
 text='\n'.join(parts)
 if r.get('truncated'):text+='\n[result truncated at the tool\'s bound]'
 return text
for folder in ['r13-four-c-binary-bound','r06-reviewer-boundaries']:
 for f in (OUT/'observations'/folder).glob('*.json'):
  d=json.loads(f.read_text());
  if 'events' not in d:continue
  events=d['events']; index={e['eventId']:e for e in events};wire=d['wire'];db=sqlite3.connect('file:'+str(Path(d['home'])/'lingxi-service/data/runs.db')+'?mode=ro',uri=True)
  count=0;result=[]
  for e in events:
   if e['eventType']!='tool_result_model_projection':continue
   p=e['payload'];raw=index[p['rawEventId']];assert raw['payload']['toolCallId']==p['toolCallId'];assert raw['eventType']=='tool_call_completed';assert int(raw['seq'])<int(e['seq']);assert p['scope']=='model_context_projection_not_raw_tool_archive'
   r=p['result'];rawr=raw['payload']['result'];text=render(r)
   if r['status']=='success':
    assert len(text.encode())<=32768
    assert p['projectionContentDigest']==hashlib.sha256(canonical(r['content'])).hexdigest()
   if rawr['status']=='success':assert p['rawContentDigest']==hashlib.sha256(canonical(rawr['content'])).hexdigest()
   # 后续实际线上内容及供应商call id逐条对应。
   if p.get('processStatus') is None:
    matches=[m for w in wire for m in w['body']['messages'] if m.get('role')=='tool' and m.get('tool_call_id')==p['providerCallId']]
    assert matches
    for m in matches:assert m['content']==text
    assert p['renderedBytes']==len(text.encode())
   row=db.execute("SELECT payload_json FROM key_events WHERE event_id=?",(e['eventId'],)).fetchone();assert row;assert json.loads(row[0])==p
   result.append({'rawEventId':p['rawEventId'],'projectionEventId':e['eventId'],'providerCallId':p['providerCallId'],'rawDigest':p['rawContentDigest'],'projectionDigest':p['projectionContentDigest'],'renderedBytes':p['renderedBytes'],'truncated':r['truncated'],'rawSeq':raw['seq'],'projectionSeq':e['seq']});count+=1
  summaries.append({'file':str(f.relative_to(OUT)),'checks':result,'projection_count':count,'real_db_verified':True,'home':d['home']})
(OUT/'independent-evidence-reconciliation.json').write_text(json.dumps(summaries,ensure_ascii=False,indent=2));print('independent reconciliation',len(summaries),'cases')
