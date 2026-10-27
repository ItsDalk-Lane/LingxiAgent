#!/usr/bin/env python3
"""当前真实服务/正式工具/Run/磁盘证据；只有远端模型端点由本脚本替身。
运行：python3 本文件 --binary 真实候选二进制 --product-dir 仓库lib --output 新证据目录
所有测试资料均由本脚本新建；不触碰用户目录。
"""
import argparse, hashlib, http.client, http.server, json, os, pathlib, queue, signal, socket, sqlite3, subprocess, threading, time


def digest(data): return hashlib.sha256(data).hexdigest()
def dump(path, value): path.write_text(json.dumps(value, ensure_ascii=False, indent=2)+'\n')

class Provider:
    def __init__(self):
        self.steps=queue.Queue(); self.requests=[]; self.lock=threading.Lock()
        outer=self
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                with outer.lock: outer.requests.append(body)
                step=outer.steps.get(timeout=90)
                if step is None:
                    delta={'role':'assistant','content':'已按真实工具结果回答。'}; finish='stop'
                else:
                    name,args,ident=step
                    delta={'role':'assistant','tool_calls':[{'index':0,'id':ident,'type':'function','function':{'name':name,'arguments':json.dumps(args,ensure_ascii=False)}}]}; finish='tool_calls'
                frames=[{'id':'chatcmpl-discovery','model':'discovery-provider','choices':[{'index':0,'delta':delta,'finish_reason':None}]}, {'id':'chatcmpl-discovery','choices':[{'index':0,'delta':{},'finish_reason':finish}]}, {'id':'chatcmpl-discovery','choices':[],'usage':{'prompt_tokens':80,'completion_tokens':30}}]
                data=(''.join('data: '+json.dumps(f,ensure_ascii=False)+'\n\n' for f in frames)+'data: [DONE]\n\n').encode()
                self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.send_header('Content-Length',str(len(data))); self.end_headers(); self.wfile.write(data)
            def log_message(self,*_): pass
        self.server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
        self.server.daemon_threads=True
        self.server.block_on_close=False
        threading.Thread(target=self.server.serve_forever,daemon=True).start()
        self.url='http://127.0.0.1:'+str(self.server.server_port)
    def close(self): self.server.shutdown(); self.server.server_close()

class Service:
    def __init__(self,binary,product,root,provider):
        self.home=root/'home'; self.home.mkdir(exist_ok=True); self.ws=root/'workspace'; self.ws.mkdir(exist_ok=True)
        self.db=self.home/'lingxi-service/data/runs.db'; self.binary=binary; self.product=product; self.root=root
        self.config=root/'service.json'
        dump(self.config,{'home':str(self.home),'workspace':str(self.ws),'providers':{'main':{'protocol':'openai-completions','endpoint':provider.url,'auth':{'kind':'apiKey','apiKey':'sk-synthetic-discovery'}}},'models':{'chat':{'provider':'main','model':'discovery-provider','capabilities':{'tools':True}}}})
    def start(self):
        env=os.environ.copy(); env.update({'LINGXI_HOME':str(self.home),'TMPDIR':str(self.root/'tmp'),'LINGXI_PRODUCT_DIR':str(self.product)})
        (self.root/'tmp').mkdir(exist_ok=True)
        self.log=(self.root/f'service-{time.time_ns()}.log').open('w')
        self.p=subprocess.Popen([str(self.binary),'--home',str(self.home),'--config',str(self.config),'--bind','127.0.0.1:0'],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=self.log,text=True)
        ready=queue.Queue()
        def read():
            for line in self.p.stdout:
                self.log.write(line); self.log.flush()
                if line.startswith('LINGXI_SERVICE_READY '): ready.put(line)
        threading.Thread(target=read,daemon=True).start()
        try: line=ready.get(timeout=35)
        except queue.Empty: raise AssertionError('真实服务未就绪；检查日志')
        self.addr=next(x[5:] for x in line.split() if x.startswith('addr='))
        self.token=json.loads((self.home/'lingxi-service/local-token.json').read_text())['token']
    def stop(self):
        self.p.send_signal(signal.SIGTERM)
        try: self.p.wait(timeout=30)
        except subprocess.TimeoutExpired: self.p.kill(); self.p.wait(); raise
        self.log.close()
    def http(self,method,path,body=None,auth=True):
        conn=http.client.HTTPConnection(self.addr,timeout=90)
        headers={'Content-Type':'application/json'}
        if auth: headers['Authorization']='Bearer '+self.token
        conn.request(method,path,None if body is None else json.dumps(body).encode(),headers)
        reply=conn.getresponse(); status=reply.status; raw=reply.read(); conn.close()
        return status,json.loads(raw or 'null')
    def sql(self,query,args=()):
        with sqlite3.connect(self.db) as con: return con.execute(query,args).fetchall()


def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--binary',type=pathlib.Path,required=True); ap.add_argument('--product-dir',type=pathlib.Path,required=True); ap.add_argument('--output',type=pathlib.Path,required=True); args=ap.parse_args()
    root=args.output.resolve(); root.mkdir(exist_ok=False,parents=True)
    provider=Provider(); service=Service(args.binary.resolve(),args.product_dir.resolve(),root,provider)
    ws=service.ws; outside=root/'outside'; outside.mkdir(); (outside/'private.txt').write_text('OUTSIDE_SECRET_DO_NOT_DISCLOSE')
    safe=ws/'safe'; safe.mkdir(); (safe/'sub').mkdir()
    original=b'\xef\xbb\xbfhello 42\r\n\xe4\xb8\xad\xe6\x96\x87 alpha-17\r\nALPHA-88\nlast\n'
    (safe/'a.txt').write_bytes(original); (safe/'sub/b.md').write_text('alpha-7\nalpha-8\n'); (safe/'.hidden').write_text('hidden'); (safe/'binary.bin').write_bytes(b'abc\x00alpha-99'); (safe/'literal.txt').write_text('a.b\naXb\n'); (safe/'mixed.TXT').write_text('case only')
    os.utime(safe/'a.txt',ns=(1700000000000000000,1700000000000000000)); os.utime(safe/'sub/b.md',ns=(1700000001000000000,1700000001000000000))
    (ws/'guard').mkdir(); (ws/'guard/escape').symlink_to(outside,target_is_directory=True)
    (ws/'unreadable').mkdir(); (ws/'unreadable/secret').write_text('alpha-44'); (ws/'unreadable').chmod(0o100)
    (ws/'unreadable-file').write_text('alpha-55'); (ws/'unreadable-file').chmod(0)
    os.mkfifo(ws/'fifo')
    cases=[]; counter=0; sid='native_discovery_readonly'
    def record(name,detail):
        cases.append({'case':name,'status':'PASS','detail':detail}); dump(root/'cases.json',cases); print('PASS',name,flush=True)
    def snapshot():
        rows={}
        for path in sorted(safe.rglob('*')):
            stat=path.stat(); rows[str(path.relative_to(ws))]={'mode':stat.st_mode,'mtime_ns':stat.st_mtime_ns,'bytes':digest(path.read_bytes()) if path.is_file() else None}
        return rows
    before=snapshot()
    def invoke(tool,arguments,success=True):
        nonlocal counter
        counter+=1; ident='discovery-call-'+str(counter); start=len(provider.requests)
        provider.steps.put(('mcp_describe_tool',{'name':tool},ident+'-definition')); provider.steps.put(('mcp_call',{'tool':tool,'arguments':arguments},ident)); provider.steps.put(None)
        status,response=service.http('POST','/lingxi/v1/sessions/'+sid+'/execute',{'input':'执行合成读取检查 '+str(counter)})
        assert status==200,(status,response)
        requests=provider.requests[start:]; dump(root/f'provider-{counter}.json',requests)
        assert len(requests)==3,(counter,len(requests),response)
        declared=[x['function']['name'] for x in requests[0]['tools']]
        assert all(x in declared for x in ['mcp_describe_tool','mcp_call']),declared
        definition=[x for x in requests[1]['messages'] if x.get('tool_call_id')==ident+'-definition']
        assert len(definition)==1 and tool in definition[0]['content'],definition
        messages=[x for x in requests[2]['messages'] if x.get('tool_call_id')==ident]
        assert len(messages)==1,messages
        raw=messages[0]['content']
        if success:
            details, consumed=json.JSONDecoder().raw_decode(raw)
            assert raw[consumed:].strip() in ('','[result truncated at the tool\'s bound]'),raw
            dump(root/f'outcome-{counter}.json',details)
            assert details['kind']==tool,details
            assert str(root) not in raw and 'OUTSIDE_SECRET' not in raw,raw
            return details
        assert raw.startswith('tool error ['),raw
        assert str(root) not in raw and 'OUTSIDE_SECRET' not in raw,raw
        dump(root/f'outcome-{counter}.json',{'errorText':raw})
        return {'errorText':raw}
    service.start()
    try:
        agent_id=service.sql("SELECT agent_id FROM sessions WHERE session_id='sess_local_alpha'")[0][0]
        status,created=service.http('POST','/lingxi/v1/sessions',{'sessionId':sid,'agentId':agent_id,'title':'原生读取合成会话','permissionMode':'read_only','memoryEnabled':False})
        assert status in (200,201),(status,created)
        record('real read-only session created through authorized product API',created)
        status,_=service.http('POST','/lingxi/v1/sessions/'+sid+'/execute',{'input':'无权'},auth=False); assert status in (401,403); assert not provider.requests
        record('unauthenticated rejected before provider',status)
        result=invoke('ls',{'path':'safe'}); entries={x['name']:x for x in result['entries']}; assert set(entries)=={'.hidden','a.txt','sub','binary.bin','literal.txt','mixed.TXT'},result
        assert entries['a.txt']['size']==len(original) and entries['sub']['isDirectory'] and result['total']==6
        assert abs(entries['a.txt']['mtimeMs']-(safe/'a.txt').stat().st_mtime_ns//1000000)<=1
        record('ls real entries dotfiles metadata',result)
        result=invoke('ls',{'path':'safe','limit':2}); assert len(result['entries'])==2 and result['total']==6 and result['truncated'] and result['nextOffset']==2; record('ls bounded explicit complete total',result)
        result=invoke('ls',{}); assert any(x['name']=='safe' for x in result['entries']); record('ls default actual workspace',result)
        result=invoke('find',{'path':'safe','pattern':'**/*.{txt,md}'}); assert set(result['paths'])=={'a.txt','literal.txt','sub/b.md','mixed.TXT'},result
        record('find recursive real glob',result)
        result=invoke('find',{'path':'safe','pattern':'*.TXT'}); assert result['paths']==['mixed.TXT']; record('find uppercase pattern remains case sensitive',result)
        result=invoke('find',{'path':'safe','pattern':'*.txt'}); assert set(result['paths'])=={'a.txt','literal.txt','mixed.TXT'}; record('find lowercase pattern uses incumbent smart case',result)
        result=invoke('find',{'path':'safe','pattern':'never-*.txt'}); assert result['paths']==[] and result['total']==0 and not result['truncated']; record('find genuine empty',result)
        result=invoke('grep',{'path':'safe/a.txt','pattern':'alpha-[0-9]+','ignoreCase':True}); matches=result['matches']; assert len(matches)==2,result
        assert matches[0]['line']==2 and matches[0]['positions'][0]=={'column':4,'endColumn':12,'byteStart':20,'byteEnd':28,'match':'alpha-17','matchTextTruncated':False},matches[0]
        assert matches[0]['sourceSha256']==digest(original) and matches[1]['line']==3 and matches[1]['positions'][0]['byteStart']==30
        record('grep true regex unicode BOM CRLF positions source digest',result)
        result=invoke('grep',{'path':'safe','pattern':'a.b','literal':True,'glob':'*.txt'}); assert [x['text'] for x in result['matches']]==['a.b']; record('grep literal actual special chars',result)
        result=invoke('grep',{'path':'safe','pattern':'alpha-[0-9]+','ignoreCase':True,'glob':'**/*.{txt,md}','output_mode':'count'}); assert result['counts']==[{'path':'a.txt','count':2},{'path':'sub/b.md','count':2}],result; record('grep count glob sorted ties',result)
        result=invoke('grep',{'path':'safe','pattern':'alpha','ignoreCase':True,'glob':'**/*.{txt,md}','output_mode':'files'}); assert result['files']==['sub/b.md','a.txt']; record('grep matching files newest first real mtime',result)
        result=invoke('grep',{'path':'safe/a.txt','pattern':'alpha','ignoreCase':True,'context':1,'limit':1}); assert result['total']==2 and result['nextOffset']==1 and len(result['matches'][0]['context'])==3; record('grep context limit explicit truncation',result)
        result=invoke('grep',{'path':'safe/a.txt','pattern':'alpha','ignoreCase':True,'offset':1}); assert len(result['matches'])==1 and result['matches'][0]['line']==3 and result['nextOffset'] is None; record('grep genuine offset',result)
        result=invoke('grep',{'path':'safe','pattern':'never-match'}); assert result['matches']==[] and result['total']==0 and result['skippedBinaryFiles']==1; record('grep genuine empty and explicit binary skip',result)
        for tool,args0 in [('ls',{'path':'missing'}),('ls',{'path':str(outside)}),('find',{'path':'safe','pattern':'['}),('grep',{'path':'safe','pattern':'['}),('grep',{'path':'safe','pattern':'alpha','offset':1,'context':1})]:
            record(tool+' refusal '+str(args0),invoke(tool,args0,False))
        for tool in ['ls','find','grep']:
            args0={'path':'guard'}
            if tool!='ls': args0['pattern']='*' if tool=='find' else 'alpha'
            record(tool+' symlink escape rejected',invoke(tool,args0,False))
            args0['path']=str(outside); record(tool+' external root rejected',invoke(tool,args0,False))
            args0['path']='not-present'; record(tool+' missing root rejected',invoke(tool,args0,False))
        for tool in ['ls','find','grep']:
            args0={'path':'unreadable'}
            if tool!='ls': args0['pattern']='*' if tool=='find' else 'alpha'
            record(tool+' actual directory IO error',invoke(tool,args0,False))
        record('grep actual file IO error',invoke('grep',{'path':'unreadable-file','pattern':'alpha'},False))
        record('grep FIFO rejected without waiting',invoke('grep',{'path':'fifo','pattern':'alpha'},False))
        assert snapshot()==before; assert (outside/'private.txt').read_text()=='OUTSIDE_SECRET_DO_NOT_DISCLOSE'
        assert service.sql('SELECT count(*) FROM session_files')[0][0]==0
        assert service.sql('SELECT count(*) FROM file_history_snapshots')[0][0]==0
        record('read only original bytes metadata and no fake file records',before)
        journals=service.sql('SELECT target,phase,receipt_outcome,receipt_detail FROM invocation_journal ORDER BY prepared_at_unix_ms'); dump(root/'journal.json',journals)
        archived=service.sql("SELECT message_id,run_id,role,content_json FROM messages WHERE role IN ('model_history','tool') ORDER BY seq"); dump(root/'model-history.json',archived); assert archived
        raw_events=service.sql("SELECT event_id,run_id,event_type,payload_json FROM key_events WHERE event_type LIKE 'tool%' ORDER BY committed_at_unix_ms,seq"); dump(root/'raw-tool-events.json',raw_events); assert raw_events
        assert any('sourceSha256' in row[3] for row in raw_events),raw_events
        service.stop(); service.start()
        provider.steps.put(None); start=len(provider.requests)
        status,response=service.http('POST','/lingxi/v1/sessions/'+sid+'/execute',{'input':'重启后继续同一真实会话'}); assert status==200,(status,response)
        restart=provider.requests[start:]; dump(root/'provider-restart.json',restart)
        assert len(restart)==1 and any(x.get('role')=='tool' for x in restart[0]['messages']),restart
        record('reboot next Run consumes persisted genuine tool history',len(restart[0]['messages']))
        slow=ws/'slow'; slow.mkdir()
        for i in range(12000): (slow/f'{i:05d}.txt').write_text('读取取消真实材料 '+str(i)+'\n')
        ident='discovery-cancel-real'; provider.steps.put(('mcp_describe_tool',{'name':'grep'},ident+'-definition')); provider.steps.put(('mcp_call',{'tool':'grep','arguments':{'path':'slow','pattern':'no-match'}},ident))
        host,port=service.addr.rsplit(':',1); transport=socket.create_connection((host,int(port)),timeout=10)
        payload=json.dumps({'input':'读取中途关闭调用连接'}).encode()
        header=(f'POST /lingxi/v1/sessions/{sid}/execute HTTP/1.1\r\nHost: {service.addr}\r\nAuthorization: Bearer {service.token}\r\nContent-Type: application/json\r\nContent-Length: {len(payload)}\r\n\r\n').encode()
        transport.sendall(header+payload)
        deadline=time.monotonic()+20; observed=None
        while time.monotonic()<deadline:
            rows=service.sql("SELECT run_id,phase FROM invocation_journal ORDER BY rowid DESC LIMIT 1")
            if rows and rows[0][1] == 'started':
                observed=rows[0]; break
            time.sleep(.002)
        assert observed,'必须真正进入读取阶段，不能以开始前取消代替'
        # 真实操作系统句柄证明阻塞扫描已物理打开目录，不能只拿日志started代替。
        deadline=time.monotonic()+3; physical=None
        while time.monotonic()<deadline:
            observed_fds=subprocess.run(['/usr/sbin/lsof','-nP','-a','-p',str(service.p.pid),'-F','n'],capture_output=True,text=True,timeout=5).stdout
            if 'n'+str(slow.resolve()) in observed_fds.splitlines():
                physical=observed_fds; break
        assert physical,'必须观测真实扫描目录句柄后才断线取消'
        (root/'cancel-physical-fds.txt').write_text(physical)
        record('OS sees real in-flight scan directory handle before cancellation',str(slow.resolve()))
        status,busy=service.http('POST','/lingxi/v1/sessions/'+sid+'/execute',{'input':'并发读取必须忙'})
        assert status==409,(status,busy)
        transport.shutdown(socket.SHUT_RDWR); transport.close()
        deadline=time.monotonic()+20; final=None
        while time.monotonic()<deadline:
            rows=service.sql('SELECT status FROM runs WHERE run_id=?',(observed[0],))
            if rows and rows[0][0] in ('cancelled','failed','completed'):
                final=rows[0][0]; break
            time.sleep(.01)
        assert final=='cancelled',(observed,final)
        receipts=service.sql('SELECT phase,receipt_outcome,receipt_detail FROM invocation_journal WHERE run_id=? AND target=\'tool:first-party:grep\'',(observed[0],))
        assert receipts and all(row[1]!='succeeded' for row in receipts),receipts
        cancel_events=service.sql('SELECT event_type,payload_json FROM key_events WHERE run_id=? ORDER BY seq',(observed[0],)); dump(root/'cancel-events.json',cancel_events)
        grep_ids=[json.loads(payload)['toolCall']['toolCallId'] for kind,payload in cancel_events if kind=='tool_call_started' and json.loads(payload)['toolCall']['target']=='tool:first-party:grep']
        assert len(grep_ids)==1 and not any(kind=='tool_call_completed' and json.loads(payload).get('toolCallId')==grep_ids[0] for kind,payload in cancel_events),cancel_events
        record('true in-flight disconnect cancellation and concurrent busy no false success',{'observed':observed,'run':final,'receipts':receipts})
        provider.steps.put(None); deadline=time.monotonic()+5; busy_count=0; start=len(provider.requests)
        while True:
            status,response=service.http('POST','/lingxi/v1/sessions/'+sid+'/execute',{'input':'实际取消后下一真实运行'})
            if status==200: break
            assert status==409 and time.monotonic()<deadline,(status,response)
            busy_count+=1; time.sleep(.01)
        assert len(provider.requests)==start+1
        record('physical worker releases session permit then next genuine Run succeeds',{'busyResponses':busy_count,'accepted':response})
        assert snapshot()==before and service.sql('SELECT count(*) FROM session_files')[0][0]==0
        dump(root/'summary.json',{'status':'PASS','cases':len(cases),'binarySha256':digest(args.binary.read_bytes()),'platform':os.uname().sysname,'providerBoundary':'external HTTP model endpoint only'})
    finally:
        if getattr(service,'p',None) and service.p.poll() is None: service.stop()
        (ws/'unreadable').chmod(0o700); (ws/'unreadable-file').chmod(0o600); provider.close()

if __name__=='__main__': main()
