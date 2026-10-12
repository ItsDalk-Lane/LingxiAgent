"""只读复查用合成探针：真实服务的知识选择在供应商自动重试后是否有效。"""
from pathlib import Path
import os, sys, json, sqlite3, re, hashlib
root=Path.cwd();out=Path(__file__).resolve().parent
(out/'tmp').mkdir(exist_ok=True)
os.environ['TMPDIR']=str(out/'tmp');os.environ['TMP']=str(out/'tmp');os.environ['TEMP']=str(out/'tmp')
import tempfile
tempfile.tempdir=str(out/'tmp')
binpath=Path('/Users/study_superior/.codex/r06-w05-r01-runtime/target/debug/lingxi-service')
fixture_path=root/'rust/crates/lingxi-service/tests/fixtures/r06_ar_f01_probe.py'
sys.argv=[str(fixture_path),str(binpath),'unused']
ns={};exec(compile(fixture_path.read_text().split('fixtures=[];checks=[]')[0],str(fixture_path),'exec'),ns)
f=ns['Fixture']('f13-retry');steps={};selections={};report={};error=None

def reply(body):
 text=next(m['content'] for m in reversed(body['messages']) if m['role']=='user')
 mode='retry' if 'RETRY_RECHECK' in text else 'control'
 step=steps.get(mode,0);steps[mode]=step+1
 if mode=='retry' and step==0:
  return [{'error':{'message':'合成供应商暂时失败'}}],500
 logical=step-(1 if mode=='retry' else 0)
 system='\n'.join(m['content'] for m in body['messages'] if m['role']=='system')
 selections[mode]=json.JSONDecoder().raw_decode(system.split('本轮知识资料范围：',1)[1])[0]
 if logical==0:return f.tool('mcp_describe_tool',{'name':'knowledge_outline'},mode+'-describe'),200
 if logical==1:return f.tool('mcp_call',{'tool':'knowledge_outline','arguments':{'scopeId':selections[mode]['scopeId']}},mode+'-outline'),200
 return f.text('结束合成检查'),200

try:
 f.reply=reply;f.start()
 nb=f.request('knowledge/notebooks',{'name':'F13合成笔记本'},expected=201)['notebook']['id']
 imported=f.request('knowledge/notebooks/'+nb+'/sources',{'kind':'pasted_text','displayName':'F13合成来源','text':'# F13\n\n仅用于知识范围重试检查。'},expected=201)
 src=imported['source']['id']
 selection={'notebookIds':[nb],'sourceIds':[src]}
 control=f.request('sessions/sess_local_alpha/execute',{'input':'CONTROL_RECHECK','knowledge':selection})
 retried=f.request('sessions/sess_local_alpha/execute',{'input':'RETRY_RECHECK','knowledge':selection})
 f.stop()
 report={'binarySHA256':hashlib.sha256(binpath.read_bytes()).hexdigest(),'fixtureSHA256':hashlib.sha256(fixture_path.read_bytes()).hexdigest(),'control':control,'retried':retried,'selections':selections,'steps':steps,'realService':f.snapshot()}
 dbpath=f.home/'knowledge/knowledge.db'
 with sqlite3.connect('file:'+str(dbpath)+'?mode=ro',uri=True) as db:
  report['scopeRows']=db.execute('SELECT id,turn_id,session_id,notebook_ids_json,status FROM knowledge_turn_scopes ORDER BY created_at,id').fetchall()
  report['scopeSources']=db.execute('SELECT * FROM knowledge_turn_scope_sources').fetchall()
 report['retryScopeRejected']=any('KNOWLEDGE_SCOPE_VIOLATION' in json.dumps(body,ensure_ascii=False) for body in f.wire if any(m.get('role')=='user' and m.get('content')=='RETRY_RECHECK' for m in body['messages']))
 report['formalAcceptance']=False
except Exception as exc:
 error=repr(exc);report={'error':error,'wire':f.wire,'http':f.http,'logs':f.logs,'syntheticHome':str(f.home)}
finally:
 f.close();(out/'result.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));print(json.dumps({'probeError':error,'retryScopeRejected':report.get('retryScopeRejected'),'steps':steps,'evidence':str(out/'result.json')},ensure_ascii=False))
