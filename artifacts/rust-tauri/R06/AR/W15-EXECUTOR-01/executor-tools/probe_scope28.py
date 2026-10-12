import pathlib,os,subprocess,json,time,urllib.request,urllib.error,base64,hashlib,signal
base=pathlib.Path(__file__).parent
root=base/'own.git'
out=pathlib.Path(os.environ['R06_T08_EVAL_OUT']).parent
home=base/'scope28-home'; home.mkdir()
env=dict(os.environ,LINGXI_HOME=str(home))
binary=base/'target/debug/lingxi-service'
proc=subprocess.Popen([str(binary),'--home',str(home),'--bind','127.0.0.1:0'],env=env,stdout=subprocess.PIPE,stderr=(out/'service.stderr').open('w'),text=True)
try:
    ready=proc.stdout.readline().strip()
    assert ready.startswith('LINGXI_SERVICE_READY '),ready
    addr=next(piece.split('=',1)[1] for piece in ready.split() if piece.startswith('addr='))
    token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
    agent=home/'agents/lingxi';agent.mkdir(parents=True,exist_ok=True)
    (agent/'config.yaml').write_text('agent:\n  name: 灵犀\nmemory:\n  enabled: true\n')
    (agent/'AGENTS.md').write_text('W15-AGENTS-original')
    (agent/'identity.md').write_text('W15-IDENTITY-original')
    (agent/'AGENTS.public.md').write_text('W15-PUBLIC-original')
    png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lVsAAAAASUVORK5CYII=')
    (agent/'avatars').mkdir(exist_ok=True);(agent/'avatars/agent.png').write_bytes(png)
    (home/'user/avatars').mkdir(parents=True,exist_ok=True);(home/'user/avatars/user.png').write_bytes(png)
    def request(method,path,body=None,extra=None):
        headers={'Authorization':'Bearer '+token,**(extra or {})}
        data=None if body is None else json.dumps(body).encode()
        if data is not None:headers['Content-Type']='application/json'
        req=urllib.request.Request('http://'+addr+path,data,headers,method=method)
        try:r=urllib.request.urlopen(req,timeout=30)
        except urllib.error.HTTPError as err:r=err
        payload=r.read();head=dict(r.headers)
        try:value=json.loads(payload)
        except (ValueError,UnicodeDecodeError):value={'bodySha256':hashlib.sha256(payload).hexdigest(),'bytes':len(payload)}
        return {'method':method,'path':path,'status':r.status,'responseHeaders':head,'response':value,'responseBodySha256':hashlib.sha256(payload).hexdigest()}
    original=json.loads((root/'docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json').read_text())['extra_root_clusters'][-1]['r00_original_assertions']
    cases=[]
    for row in original:
        fid=row['feature_id'];suffix=fid.rsplit('-',1)[1];path='/lingxi/v1/agents/lingxi'
        mapping={
          'A02D8C':('GET',path+'/avatar-path',None),
          '8C8083':('PATCH',path+'/config',{'chat':{'layout':'audit'}}),
          '517E69':('DELETE','/lingxi/v1/agents/another',None),
          '3516A7':('DELETE','/lingxi/v1/agents/another',{'deleteSkills':['synthetic_skill']}),
          '0B483D':('PATCH',path+'/config',{'memory':{'enabled':False}}),
          '487080':('PUT',path+'/agents-md',{'content':'W15-AGENTS-updated'}),
          '07A5CE':('GET',path+'/agents-md',None),
          'BB4CD1':('DELETE',path+'/avatar',None),
          '7F0F3F':('POST',path+'/avatar',{'dataUrl':'data:image/png;base64,'+base64.b64encode(png).decode()}),
          'CCDCE8':('GET',path+'/avatar',None),
          'BC675C':('GET',path+'/config',None),
          '856B28':('PUT',path+'/identity',{'content':'W15-IDENTITY-updated'}),
          'A9B0BB':('GET',path+'/identity',None),
          '3D7A95':('GET',path+'/public-agents-md',None),
          '7C699F':('PUT',path+'/public-agents-md',{'content':'W15-PUBLIC-updated'}),
          '8D4356':('PUT','/lingxi/v1/agents/order',{'order':['lingxi']}),
          '9CC1B1':('POST','/lingxi/v1/agents',{'id':'synthetic-new','name':'合成助手'}),
          'C0ADBA':('PUT','/lingxi/v1/agents/primary',{'id':'lingxi'}),
          '26AE36':('GET','/lingxi/v1/agents?fresh=1',None),
          '8B48FD':('POST','/lingxi/v1/agents/switch',{'id':'lingxi'}),
          '4A56F2':('DELETE','/lingxi/v1/avatar/user',None),
          '7570BE':('POST','/lingxi/v1/avatar/user',{'dataUrl':'data:image/png;base64,'+base64.b64encode(png).decode()}),
          'E4BEE8':('GET',path+'/avatar',None),
          '24BC2E':('PATCH',path+'/config',{'keep_awake':True}),
          '23172C':('PATCH',path+'/config',{'workspace_context':{'discover_project_skills':False}}),
          '38A989':('PATCH',path+'/config',{'providers':{'audit':None}}),
          '9E3981':('PATCH',path+'/config',{'providers':{'audit':{'endpoint':'http://127.0.0.1:1'}}}),
          '464E58':('PATCH',path+'/config',{'api':{'provider':'audit','apiKey':'sk-synthetic-local-only'}}),
        }
        args=mapping[suffix];observations=[request(*args)]
        if suffix=='E4BEE8':
            etag=observations[0]['responseHeaders'].get('ETag')
            observations.append(request('GET',path+'/avatar',extra={'If-None-Match':etag or '"missing-etag-probe"'}))
        files=[{'path':str(p.relative_to(home)),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in agent.rglob('*') if p.is_file()]
        cases.append({'featureId':fid,'originalAssertions':row['assertions'],'originalDeadline':'R06 remains due; R07-T08 general management API is not a waiver','observations':observations,'files':files,'fullBehaviorStatus':'OPEN_NOT_CLOSED_BY_W15','scopeDecision':'Original R06-only deadline preserved; source overlap with R07-T08 requires independent review, no unilateral deferral'})
    (out/'scope28-observations.json').write_text(json.dumps({'ready':ready,'pid':proc.pid,'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'effectiveLingxiHome':str(home),'osHomeInherited':env['HOME'],'productDir':env['LINGXI_PRODUCT_DIR'],'cases':cases,'notFullSemanticCoverage':True},ensure_ascii=False,indent=2))
    missing=[case['featureId'] for case in cases if case['observations'][0]['status'] in (404,405)]
    print(json.dumps({'originalLeaves':len(cases),'missingCurrentManagementOrRoleRoutes':missing,'avatarEtagPresent':bool(next(c for c in cases if c['featureId'].endswith('E4BEE8'))['observations'][0]['responseHeaders'].get('ETag')),'status':'OPEN_SOURCE_AND_RUNTIME_READBACK_NOT_FULL_ACCEPTANCE'},ensure_ascii=False))
finally:
    proc.send_signal(signal.SIGTERM)
    code=proc.wait(timeout=30)
    (out/'service-exit.json').write_text(json.dumps({'pid':proc.pid,'exitCode':code,'homePreservedLocalOnly':str(home)}))
