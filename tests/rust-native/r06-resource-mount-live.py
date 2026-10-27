# 真实CLI、原TS挂载登记、HTTP和磁盘共同核验；不替换资源内核。
import base64, hashlib, json, os, pathlib, re, subprocess, time, urllib.request, urllib.error
root = pathlib.Path(os.environ['R06_EX10_EVIDENCE']); root.mkdir(parents=True); root = root.resolve()
home = root/'home'; workspace = root/'workspace'; mount = root/'mount'
for path in (home,workspace,mount): path.mkdir()
(root/'config.json').write_text(json.dumps({'home':str(home),'workspace':str(workspace)}))
log = (root/'service.log').open('w'); trace=[]; checks=[]
proc = subprocess.Popen([os.environ['R06_SERVICE_BIN'],'--config',str(root/'config.json'),'--bind','127.0.0.1:0'],env={**os.environ,'LINGXI_HOME':str(home),'TMPDIR':str(root)},stdout=subprocess.PIPE,stderr=log,text=True)
try:
 url=None
 for line in proc.stdout:
  log.write(line); log.flush(); match=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
  if match: url='http://'+match[1]; break
 assert url, '真实服务未就绪'
 token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
 def api(method,path,body=None,auth=True):
  request=urllib.request.Request(url+path,data=json.dumps(body).encode() if body is not None else None,method=method,headers={'Content-Type':'application/json',**({'Authorization':'Bearer '+token} if auth else {})})
  try: response=urllib.request.urlopen(request,timeout=15)
  except urllib.error.HTTPError as error: response=error
  payload=json.loads(response.read()); trace.append({'method':method,'path':path,'body':body,'status':response.status,'response':payload}); return response.status,payload
 def expect(status,result,expected=200):
  assert status==expected, (status,result,expected); return result
 def post(action,body,expected=200): return expect(*api('POST','/lingxi/v1/resource-io/'+action,body),expected)
 def ref(path=''): return {'kind':'mount','mountId':'configured-real-mount','path':path}
 def reason(value): return value.get('details',{}).get('reason',value.get('code'))
 def check(name): checks.append(name)
 identity=expect(*api('GET','/lingxi/v1/server/identity'))
 registry_script="import {upsertStudioMount} from './core/studio-mounts.ts'; const [home,studio,root]=process.argv.slice(1);upsertStudioMount(home,{mountId:'configured-real-mount',hostStudioId:studio,sourceKind:'storage',provider:'local_fs',rootLocator:{path:root},label:'真实合成挂载',presentation:'folder',capabilities:['list','read','write','watch','materialize']});"
 configured=subprocess.run([os.environ['R06_NODE_BIN'],'--input-type=module','-e',registry_script,str(home),identity['studioId'],str(mount)],cwd=os.environ['R06_REPOSITORY'],capture_output=True,text=True)
 (root/'configuration.log').write_text(configured.stdout+configured.stderr); assert configured.returncode==0, configured.stderr
 registry_path=home/'studio-mounts.json'; original_registry=registry_path.read_bytes()
 assert post('stat',{'resource':ref('absent.txt')})['exists'] is False
 out=post('write',{'resource':ref('sub/note.txt'),'content':'hello 挂载\nsecond line'})
 assert out['resource']['kind']=='mount' and out['resourceKey']=='mount:configured-real-mount:sub/note.txt'
 assert (mount/'sub/note.txt').read_text()=='hello 挂载\nsecond line'; check('write_real_disk_and_mount_identity')
 stat=post('stat',{'resource':ref('sub/note.txt')}); assert stat['exists'] and stat['version']['size']==len((mount/'sub/note.txt').read_bytes()); check('stat_real_metadata')
 assert post('read',{'resource':ref('sub/note.txt')})['content']=='hello 挂载\nsecond line'
 raw=b'\xff\xfe\x00\x01'; (mount/'binary.bin').write_bytes(raw)
 post('write',{'resource':ref('binary.bin'),'encoding':'base64','content':base64.b64encode(raw).decode()}); assert (mount/'binary.bin').read_bytes()==raw
 post('write',{'resource':ref('binary.bin'),'encoding':'base64','content':'!!!'},400); assert (mount/'binary.bin').read_bytes()==raw
 assert base64.b64decode(post('read',{'resource':ref('binary.bin'),'encoding':'base64'})['content'])==raw
 assert reason(post('read',{'resource':ref('binary.bin')},400))=='invalid_resource_encoding'; check('read_utf8_binary_base64_and_encoding_failure')
 post('write-expected-version',{'resource':ref('sub/note.txt'),'content':'changed','expectedVersion':stat['version']})
 old=(mount/'sub/note.txt').read_bytes()
 conflict=post('write-expected-version',{'resource':ref('sub/note.txt'),'content':'overwrite-forbidden','expectedVersion':stat['version']},409)
 assert conflict['conflict'] and (mount/'sub/note.txt').read_bytes()==old; check('expected_version_conflict_preserves_external_bytes')
 assert [entry['name'] for entry in post('list',{'resource':ref()})['items']]==['binary.bin','sub']
 assert post('search',{'resource':ref(),'query':'changed'})['matches'][0]['text']=='changed'
 check('list_and_scoped_text_search')
 watch=post('watch',{'resource':ref()})['watchId']; subscription=post('subscribe',{'resources':[ref()],'purpose':'actual-mount-probe'})['subscriptionId']
 diagnostics=expect(*api('GET','/lingxi/v1/resource-io/watch-diagnostics'))['diagnostics']
 assert diagnostics['subscriptions']==1 and diagnostics['watches'][0]['refCount']==2
 before=(mount/'sub/note.txt').read_bytes(); assert before==old; check('watch_subscribe_diagnostics_actual_references')
 cursor=expect(*api('GET','/lingxi/v1/resource-io/events?since=0'))['latestSequence']
 (mount/'watched.txt').write_text('external-change')
 events=[]
 for _ in range(80):
  events=expect(*api('GET',f'/lingxi/v1/resource-io/events?since={cursor}'))['events']
  if any(e.get('resourceKey')=='mount:configured-real-mount:watched.txt' for e in events): break
  time.sleep(.025)
 assert any(e.get('resourceKey')=='mount:configured-real-mount:watched.txt' and e['resource']['kind']=='mount' for e in events), events
 check('watch_real_external_change_mount_event_and_cursor')
 assert expect(*api('DELETE','/lingxi/v1/resource-io/subscriptions/'+subscription))['released'] is True
 assert expect(*api('DELETE','/lingxi/v1/resource-io/subscriptions/'+subscription))['released'] is False
 assert expect(*api('DELETE','/lingxi/v1/resource-io/watch/'+watch))['released'] is True
 assert expect(*api('DELETE','/lingxi/v1/resource-io/watch/'+watch))['released'] is False; check('unsubscribe_unwatch_repeated_false')
 renamed=post('rename',{'from':ref('sub/note.txt'),'to':ref('sub/renamed.txt')}); assert renamed['newResource']['path']=='sub/renamed.txt' and not (mount/'sub/note.txt').exists()
 post('move',{'from':ref('sub/renamed.txt'),'to':ref('new/deep/moved.txt')}); assert (mount/'new/deep/moved.txt').read_bytes()==old; check('rename_move_real_paths_and_created_parent')
 (mount/'occupied.txt').write_text('KEEP')
 assert reason(post('move',{'from':ref('new/deep/moved.txt'),'to':ref('occupied.txt')},409))=='target_already_exists'
 assert (mount/'occupied.txt').read_text()=='KEEP' and (mount/'new/deep/moved.txt').read_bytes()==old
 post('move',{'from':ref('new/deep/moved.txt'),'to':{'kind':'local-file','path':str(workspace/'escape.txt')}},501)
 post('move',{'from':ref('new/deep/moved.txt'),'to':{'kind':'mount','mountId':'other','path':'escape.txt'}},501)
 assert not (workspace/'escape.txt').exists(); check('occupied_cross_provider_cross_mount_zero_side_effect')
 trash=post('trash',{'resource':ref('new/deep/moved.txt'),'trash':{'namespace':'actual-proof','metadata':{'proof':'real'}}})
 assert not (mount/'new/deep/moved.txt').exists() and pathlib.Path(trash['payloadPath']).read_bytes()==old
 assert json.loads((pathlib.Path(trash['trashPath'])/'metadata.json').read_text())['proof']=='real'; check('trash_actual_payload_and_metadata')
 denied=post('read',{'resource':{'kind':'local-file','path':str(mount/'occupied.txt')}},403); assert reason(denied)=='resource_access_denied'
 outside=root/'outside-secret.txt'; outside.write_text('PRIVATE')
 os.symlink(outside,mount/'escape-link.txt')
 post('read',{'resource':ref('escape-link.txt')},403)
 assert not any(i['name']=='escape-link.txt' for i in post('list',{'resource':ref()})['items'])
 for bad in ['../outside-secret.txt','sub/../../outside-secret.txt','C:/escape']:
  post('write',{'resource':ref(bad),'content':'forbidden'},400)
 assert outside.read_text()=='PRIVATE'; check('mount_not_global_grant_traversal_link_outside_rejected')
 for reference in [{'kind':'session-file','fileId':'sf-missing'},{'kind':'url','url':'https://example.invalid/'}]:
  post('list',{'resource':reference},403 if reference['kind']=='session-file' else 400)
  post('watch',{'resource':reference},403 if reference['kind']=='session-file' else 400)
 check('unsupported_operation_provider_explicit_failure')
 mounted_watch=post('watch',{'resource':ref()})['watchId']; mounted_sub=post('subscribe',{'resources':[ref()]})['subscriptionId']
 registry=json.loads(original_registry); registry['mounts'][0]['capabilities']=['read','list','materialize']; registry_path.write_text(json.dumps(registry))
 post('write',{'resource':ref('occupied.txt'),'content':'denied'},403)
 for _ in range(80):
  diagnostics=expect(*api('GET','/lingxi/v1/resource-io/watch-diagnostics'))['diagnostics']
  if not diagnostics['watches']: break
  time.sleep(.025)
 assert diagnostics['watches']==[] and diagnostics['subscriptions']==0 and diagnostics['lastErrorCode']=='capability_denied'
 assert expect(*api('DELETE','/lingxi/v1/resource-io/watch/'+mounted_watch))['released'] is False
 assert expect(*api('DELETE','/lingxi/v1/resource-io/subscriptions/'+mounted_sub))['released'] is False
 assert (mount/'occupied.txt').read_text()=='KEEP'; check('real_capability_revocation_stops_watch_subscriptions_and_write')
 registry['mounts'][0]['status']='disabled'; registry_path.write_text(json.dumps(registry)); post('read',{'resource':ref('occupied.txt')},404)
 registry['mounts'][0]['status']='active'; registry['mounts'][0]['hostStudioId']='foreign-studio'; registry_path.write_text(json.dumps(registry)); post('read',{'resource':ref('occupied.txt')},404)
 registry['mounts'][0]['hostStudioId']=identity['studioId']; registry['mounts'][0]['provider']='s3'; registry_path.write_text(json.dumps(registry)); post('read',{'resource':ref('occupied.txt')},501)
 registry_path.write_text('{broken'); post('list',{'resource':ref()},503); registry_path.write_bytes(original_registry)
 check('disabled_foreign_studio_unavailable_remote_corrupt_registry_fail_loudly')
 large=mount/'bounded.bin'
 with large.open('wb') as f: f.truncate(20*1024*1024+1)
 post('read',{'resource':ref('bounded.bin'),'encoding':'base64'},413); assert large.stat().st_size==20*1024*1024+1
 expect(*api('POST','/lingxi/v1/resource-io/read',{'resource':ref('occupied.txt')},False),401)
 check('bounded_read_and_auth_before_payload')
 result={'passed':True,'checks':checks,'checkCount':len(checks),'trace':trace,'registrySha256':hashlib.sha256(original_registry).hexdigest(),'outsideUnchanged':outside.read_text()=='PRIVATE'}
 (root/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)); print(json.dumps({'passed':True,'checkCount':len(checks),'evidence':str(root)},ensure_ascii=False))
finally:
 (root/'trace.json').write_text(json.dumps(trace,ensure_ascii=False,indent=2)); proc.terminate(); proc.wait(timeout=15); log.close()
