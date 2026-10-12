# 真binary/认证HTTP坏前缀与无关坏后缀探针，外部模型只用loopback。
import sys
from pathlib import Path
probe=Path('rust/crates/lingxi-service/tests/fixtures/r06_ar_f01_probe.py').read_text()
prefix,ending=probe.split('try:\n globals()[CASE]();status=',1)
code=r'''
def deep_fork_controls():
 outcomes=[]
 for mode in ['tool_pair','opaque_origin_missing','opaque_origin_forged','actor']:
  f=make('deep-fork-'+mode,protocol='anthropic-messages' if mode.startswith('opaque') else 'openai-completions');(f.ws/'deep.txt').write_text('TOOL-'+nonce);issued=[False]
  def reply(body):
   if not issued[0]:
    issued[0]=True
    if mode.startswith('opaque'):
     return [{'type':'message_start','message':{'id':'msg_signed','type':'message','role':'assistant','content':[],'model':'fixture-model','usage':{'input_tokens':10,'output_tokens':0}}},{'type':'content_block_start','index':0,'content_block':{'type':'thinking','thinking':'reason-'+nonce,'signature':'signed-'+nonce}},{'type':'content_block_stop','index':0},{'type':'content_block_start','index':1,'content_block':{'type':'text','text':'SIGNED-FINAL-'+nonce}},{'type':'content_block_stop','index':1},{'type':'message_delta','delta':{'stop_reason':'end_turn','stop_sequence':None},'usage':{'output_tokens':3}},{'type':'message_stop'}],200
    return f.tool('read',{'path':'deep.txt'}),200
   return f.normal(body)
  f.reply=reply;r=f.run('实际首轮-'+nonce);f.stop()
  with sqlite3.connect(f.home/'lingxi-service/data/runs.db') as db:
   rows=db.execute("SELECT message_id,content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1' ORDER BY seq").fetchall()
   for mid,raw in rows:
    v=json.loads(raw);item=v['payload']['items'][0]
    change=False
    if mode=='tool_pair' and item['kind']=='tool':item['providerId']='forged-provider';v['payload']['projection']['providerCallId']='forged-provider';change=True
    if mode.startswith('opaque') and item['kind']=='assistant' and any(c['type']=='opaque' for c in item['content']):item['origin']=None if mode.endswith('missing') else {'provider':'forged-provider','model':'forged-model'};change=True
    if mode=='actor':v['actorKind']='web_session';v['actorSubject']='user_local';change=True
    if change:
     v.pop('digest');v['digest']=hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest();db.execute('UPDATE messages SET content_json=? WHERE message_id=?',(json.dumps(v,ensure_ascii=False),mid))
  f.start();before=len(f.wire)
  try:f.request('sessions/sess_local_alpha/fork',{'newSessionId':'bad_'+mode,'boundaryMessageId':r['runId']+'-final'},expected=500)
  except AssertionError:pass
  status=f.http[-1]['status'];created=bool(f.sql('SELECT session_id FROM sessions WHERE session_id=?',('bad_'+mode,)));outcomes.append({'mode':mode,'status':status,'targetCreated':created,'wireBefore':before,'wireAfter':len(f.wire)});checks.append(outcomes[-1])
 g=make('deep-fork-suffix');first=g.run('PREFIX-'+nonce);second=g.run('BAD-SUFFIX-'+nonce);g.stop()
 with sqlite3.connect(g.home/'lingxi-service/data/runs.db') as db:
  mid,raw=db.execute("SELECT message_id,content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1' AND content_json LIKE ?",('%'+second['runId']+'%',)).fetchone();v=json.loads(raw);v['actorKind']='web_session';v['actorSubject']='user_local';v.pop('digest');v['digest']=hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest();db.execute('UPDATE messages SET content_json=? WHERE message_id=?',(json.dumps(v),mid))
 g.start();g.request('sessions/sess_local_alpha/fork',{'newSessionId':'good_prefix','boundaryMessageId':first['runId']+'-final'});g.run('合法保留前缀','good_prefix');assert has(g.wire[-1],'PREFIX-'+nonce) and not has(g.wire[-1],'BAD-SUFFIX-'+nonce);checks.append('bad suffix outside boundary does not reject valid prefix')
 g.request('sessions/archive',{'sessionId':'sess_local_alpha','childMode':'detach_children'});g.request('sessions/archived/delete',{'sessionId':'sess_local_alpha'});g.request('sessions/good_prefix/fork',{'newSessionId':'source_deleted_good','boundaryMessageId':first['runId']+'-final'});g.run('源删除后再次分叉','source_deleted_good');assert has(g.wire[-1],'PREFIX-'+nonce);checks.append('source-deleted self-contained archive remains valid on next authorized fork')
 assert all(v['status']==500 and not v['targetCreated'] and v['wireBefore']==v['wireAfter'] for v in outcomes),outcomes
'''
exec(prefix+code+"\ntry:\n globals()[CASE]();status="+ending)
