from capture import *
import signal, tempfile, time, urllib.request, urllib.error, shutil

out=EV/(sys.argv[1] if len(sys.argv)>1 else 'live-correlation');out.mkdir(exist_ok=False)
home=pathlib.Path(tempfile.mkdtemp(prefix='h48.',dir='/tmp'))
binary=ROOT/'rust/target/debug/lingxi-service'
env=os.environ.copy();env.update({'RUST_LOG':'debug','HOME':str(home),'LINGXI_HOME':str(home)})
for k in ['ALL_PROXY','all_proxy','HTTP_PROXY','http_proxy','HTTPS_PROXY','https_proxy']:env.pop(k,None)
argv=[str(binary),'--home',str(home),'--bind','127.0.0.1:0'];start=utc()
stdout=(out/'service.stdout.log').open('wb');stderr=(out/'service.stderr.log').open('wb')
p=subprocess.Popen(argv,cwd=ROOT,env=env,stdout=stdout,stderr=stderr)
rows=[];opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
try:
    for _ in range(300):
        match=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',(out/'service.stdout.log').read_text())
        if match:addr=match[1];break
        if p.poll() is not None:raise RuntimeError('真实服务未就绪')
        time.sleep(.05)
    else:raise RuntimeError('真实服务就绪超时')
    token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
    spoof='req-0123456789abcdef0123456789abcdef'
    cases=[('auth-bad','/lingxi/v1/me',{'Authorization':'Bearer '+spoof},401,'LINGXI_AUTH_REJECTED'),
           ('auth-missing','/lingxi/v1/me',{},401,'LINGXI_AUTH_REJECTED'),
           ('ws-auth','/lingxi/v1/ws?token='+spoof,{},401,'LINGXI_AUTH_REJECTED'),
           ('ws-transport','/lingxi/v1/ws',{'Authorization':'Bearer '+token},400,'LINGXI_TRANSPORT_REJECTED'),
           ('origin-transport','/lingxi/v1/me',{'Authorization':'Bearer '+token,'Origin':'https://untrusted.invalid'},403,'LINGXI_TRANSPORT_REJECTED')]
    for name,path,headers,expected,marker in cases:
        headers['X-Request-Id']=spoof
        req=urllib.request.Request('http://'+addr+path,headers=headers)
        t=utc()
        try:r=opener.open(req,timeout=5);status=r.status;body=r.read()
        except urllib.error.HTTPError as err:status=err.code;body=err.read()
        (out/f'{name}.body.json').write_bytes(body)
        rid=json.loads(body)['details']['requestId']
        rows.append({'case':name,'method':'GET','path':path.split('?')[0],'status':status,'expected':expected,'requestId':rid,'assignmentBytes':len('request_id='+rid),'marker':marker,'UTC_start':t,'UTC_end':utc()})
        assert status==expected,(name,status,body)
        assert re.fullmatch(r'req-[0-9a-f]{32}',rid) and rid!=spoof
finally:
    if p.poll() is None:p.send_signal(signal.SIGTERM)
    try:rc=p.wait(timeout=10)
    except subprocess.TimeoutExpired:p.kill();rc=p.wait(timeout=5)
    stdout.close();stderr.close()
    shutil.copytree(home/'lingxi-service/logs',out/'retained-logs')
    write(out/'service.command.json',{'argv':argv,'cwd':str(ROOT),'UTC_start':start,'UTC_end':utc(),'exit':rc,'pid':p.pid,'reaped':p.poll() is not None,'binarySHA256':sha(binary),'logs':{f.name:sha(f) for f in (out/'retained-logs').glob('*.log')}})
    shutil.rmtree(home)
err=(out/'service.stderr.log').read_text();logs=''.join(f.read_text() for f in (out/'retained-logs').glob('*.log'))
for row in rows:
    rid=row['requestId']
    row['realMarkerMatch']=any(row['marker'] in line and rid in line for line in err.splitlines())
    row['realHandledMatch']=any('request handled' in line and rid in line for line in logs.splitlines())
    assert row['realMarkerMatch'] and row['realHandledMatch'],row
assert spoof not in err and spoof not in logs,'伪造请求编号作为合成认证秘密不得泄露'
assert len({r['requestId'] for r in rows})==len(rows)
write(out/'result.json',{'cases':rows,'actual':len(rows),'failed':0,'cleanupExit':rc,'homeRemoved':not home.exists(),'boundary':'正式Cargo生成的main；真实回环HTTP、AUTH/TRANSPORT/WS与轮转日志；无模型或外部调用'})
print({'actual':len(rows),'failed':0,'markerAndHandledAllMatch':True,'cleanupExit':rc})
