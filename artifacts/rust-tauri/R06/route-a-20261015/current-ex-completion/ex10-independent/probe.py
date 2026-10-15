import pathlib, os, json, subprocess, urllib.request, urllib.error, time, re, hashlib
root=pathlib.Path('/private/tmp/r06-ex10-fresh-review')
fx=root/'runtime';fx.mkdir(exist_ok=True)
home=fx/'home';workspace=fx/'workspace';mount=fx/'mount'
for p in [home,workspace,mount]:p.mkdir(exist_ok=True)
config=fx/'config.json';config.write_text(json.dumps({'home':str(home),'workspace':str(workspace)}))
binpath=pathlib.Path('/Users/study_superior/.cache/lingxi-r06-builds/native-write-clean/debug/lingxi-service')
log=(fx/'service.log').open('w');trace=[]
p=subprocess.Popen([str(binpath),'--config',str(config),'--bind','127.0.0.1:0'],env={**os.environ,'LINGXI_HOME':str(home),'TMPDIR':str(fx)},stdout=subprocess.PIPE,stderr=log,text=True)
try:
 for line in p.stdout:
  log.write(line);log.flush();m=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
  if m: url='http://'+m[1];break
 token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
 def api(method,path,body=None):
  req=urllib.request.Request(url+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
  try:resp=urllib.request.urlopen(req,timeout=10)
  except urllib.error.HTTPError as e:resp=e
  val=json.loads(resp.read());trace.append({'method':method,'path':path,'body':body,'status':resp.status,'response':val});assert resp.status==200,trace[-1];return val
 studio=api('GET','/lingxi/v1/server/identity')['studioId']
 js="import {upsertStudioMount} from './core/studio-mounts.ts';const [home,studio,root]=process.argv.slice(1);upsertStudioMount(home,{mountId:'independent',hostStudioId:studio,sourceKind:'storage',provider:'local_fs',rootLocator:{path:root},label:'独立复核',presentation:'folder',capabilities:['read','write','list','watch','materialize']});"
 subprocess.run(['/Users/study_superior/.nvm/versions/node/v24.16.0/bin/node','--input-type=module','-e',js,str(home),studio,str(mount)],cwd='/Users/study_superior/Desktop/Code/LingxiAgent',check=True,capture_output=True)
 ref={'kind':'mount','mountId':'independent','path':''}
 (mount/'original.txt').write_text('original synthetic')
 watch=api('POST','/lingxi/v1/resource-io/watch',{'resource':ref})['watchId']
 sub=api('POST','/lingxi/v1/resource-io/subscribe',{'resources':[ref]})['subscriptionId']
 diag=api('GET','/lingxi/v1/resource-io/watch-diagnostics')['diagnostics'];assert diag['subscriptions']==1 and diag['watches'][0]['refCount']==2
 cursor=api('GET','/lingxi/v1/resource-io/events?since=0')['latestSequence']
 mount.rename(fx/'retained-original');mount.mkdir();(mount/'new-root-secret.txt').write_text('NEW-ROOT-SYNTHETIC')
 for _ in range(120):
  diag=api('GET','/lingxi/v1/resource-io/watch-diagnostics')['diagnostics']
  if not diag['watches']:break
  time.sleep(.025)
 assert diag['watches']==[] and diag['subscriptions']==0 and diag['lastErrorCode']=='mount_root_changed',diag
 events=api('GET',f'/lingxi/v1/resource-io/events?since={cursor}')['events']
 assert not any('new-root-secret' in json.dumps(e) for e in events),events
 assert api('DELETE','/lingxi/v1/resource-io/watch/'+watch)['released'] is False
 assert api('DELETE','/lingxi/v1/resource-io/subscriptions/'+sub)['released'] is False
 # 新请求必须沿当前登记重新获得租约；旧失效轮询不得误清理新监听。
 new_watch=api('POST','/lingxi/v1/resource-io/watch',{'resource':ref})['watchId']
 new_cursor=api('GET','/lingxi/v1/resource-io/events?since=0')['latestSequence']
 (mount/'after-reauthorize.txt').write_text('SAFE-CURRENT-ROOT')
 for _ in range(120):
  events=api('GET',f'/lingxi/v1/resource-io/events?since={new_cursor}')['events']
  if any(e.get('resourceKey')=='mount:independent:after-reauthorize.txt' for e in events):break
  time.sleep(.025)
 assert any(e.get('resourceKey')=='mount:independent:after-reauthorize.txt' for e in events),events
 assert api('GET','/lingxi/v1/resource-io/watch-diagnostics')['diagnostics']['watches'][0]['refCount']==1
 assert api('DELETE','/lingxi/v1/resource-io/watch/'+new_watch)['released'] is True
 result={'passed':True,'checks':['same_path_root_inode_replacement_releases_old_watch_and_subscription','old_watch_emits_no_new_root_entry','released_old_ids_return_false','fresh_authorized_watch_survives_old_generation_and_emits_actual_disk_event'],'binary':str(binpath),'binarySha256':hashlib.sha256(binpath.read_bytes()).hexdigest()}
 (root/'probe-result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result))
finally:
 (root/'probe-trace.json').write_text(json.dumps(trace,ensure_ascii=False,indent=2));p.terminate();p.wait(timeout=15);log.close()
