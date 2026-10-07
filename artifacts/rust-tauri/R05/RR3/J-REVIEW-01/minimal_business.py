import sys,os,subprocess,json,time,signal,importlib.util,hashlib
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parent));import review_driver as d
ROOT,EV,COPY=d.ROOT,d.EV,d.COPY
binary=ROOT/'rust/target/debug/lingxi-service'
assert d.sha(binary)=='7fa13a3bc7ad8d8fde1d55d33242f0eca8b17913172daf8fe513d899c224cd7e'
source=json.loads((ROOT/'artifacts/rust-tauri/R05/RR3/H-REVIEW-02/FINAL_SOURCE_BINDING.json').read_text())['inputHashes']
checks={p:{'expected':h,'main':d.sha(ROOT/p),'copy':d.sha(COPY/p)} for p,h in source.items()}
assert all(v['expected']==v['main']==v['copy'] for v in checks.values())
d.save('H02-source-binary-reuse.json',{'binary':str(binary),'binarySha256':d.sha(binary),'inputs':checks,'scope':'exact prior current-source binary; no rebuild/no mutant'})
assert d.run('client',['python3','-B','scripts/rust-tauri/r02_client_leaf_matrix.py',EV/'CLIENT'],COPY)==0
# 最小运行保留原脚本全部HTTP/WS/CLI业务；仅复用已逐输入核对的正式产物，未将其称完整build gate。
s=(COPY/'scripts/rust-tauri/r02_t03_auth_matrix.sh').read_text()
s=s.replace('cd "$(dirname "$0")/../.."','cd "$J_REVIEW_COPY"',1)
start=s.index('$CARGO build --manifest-path');end=s.index('BIN="$TARGET_DIR/debug/lingxi-service"',start)
s=s[:start]+'printf "%s\\n" "Independent review: exact H02 current-source binary reused; no Cargo build" > "$EVIDENCE_DIR/build.log"\n'+s[end:]
script=EV/'auth-business-existing-binary.sh';script.write_text(s)
env=d.ENV.copy();env['J_REVIEW_COPY']=str(COPY)
assert d.run('auth-business',['bash',script,EV/'AUTH'],COPY,env)==0
sys.path.insert(0,str(COPY/'scripts/rust-tauri'))
import r02_cli_sessions_leaf_matrix as sessions
result=json.loads((EV/'AUTH/sessions-list-matrix.json').read_text())
assert set(x['case'] for x in result['cases'])==sessions.EXPECTED
assert len(result['cases'])==10 and all(x['ok'] and x['actual']==x['expect'] for x in result['cases'])
d.save('cli-sessions-independent.json',{'status':'PASS','caseCount':10,'source':'AUTH/sessions-list-matrix.json','expectedCases':sorted(sessions.EXPECTED),'sourceSha256':d.sha(COPY/'scripts/rust-tauri/r02_cli_sessions_leaf_matrix.py'),'boundary':'same actual auth business result rechecked against unchanged producer exact identities; no full producer/build claimed'})
# 从原CLI_RUST执行器复用最小真实请求/判定工具，启动正式CLI和同一服务。
import r02_cli_rust_matrix as m
out=EV/'CLI_RUST_MIN';out.mkdir();home=out/'home';home.mkdir();matrix=m.Matrix(out)
env=d.ENV.copy();env.update(LINGXI_HOME=str(home),LINGXI_SERVICE_BIN=str(binary))
argv=m.cli_args('serve','--channel','stable','--','--home',str(home),'--bind','127.0.0.1:0')
fout=(out/'serve.stdout.log').open('w');ferr=(out/'serve.stderr.log').open('w')
p=subprocess.Popen(argv,cwd=COPY,env=env,stdout=fout,stderr=ferr,start_new_session=True)
addr='';pid=0;birth=''
try:
 deadline=time.monotonic()+25
 while time.monotonic()<deadline and p.poll() is None:
  f=home/'lingxi-service/instance.json'
  if f.exists():
   try:
    data=json.loads(f.read_text());addr=data.get('bindAddr','');pid=data.get('pid',0)
    if addr and m.health_ok(addr):break
   except (ValueError,OSError):pass
  time.sleep(.1)
 birth=m.process_field(pid,'lstart') if pid else ''
 matrix.case('cli-rust-serve-ready',bool(addr and m.health_ok(addr) and birth and m.process_field(pid,'ppid')==str(p.pid)),{'argv':argv,'addr':addr,'cliPid':p.pid,'rustPid':pid,'birth':birth})
 assert matrix.cases[-1]['ok']
 db=home/'lingxi-service/data/runs.db';before=m.table_counts(db)
 owner=matrix.run('sessions-owner',m.cli_args('sessions'),env=env)
 matrix.case('cli-rust-sessions-owner',owner.returncode==0 and 'Synthetic session alpha' in owner.stdout and 'Synthetic session beta' in owner.stdout,{'exit':owner.returncode})
 missing=matrix.run('continue-missing',m.cli_args('continue','does-not-exist'),env=env)
 matrix.case('cli-rust-continue-missing-no-create',missing.returncode not in (0,124) and 'Session not found' in missing.stderr and before==m.table_counts(db),{'exit':missing.returncode,'before':before,'after':m.table_counts(db)})
 bad=matrix.run('sessions-unauthorized',m.cli_args('sessions','--url','http://'+addr,'--token','intentionally-invalid-token'),env=env)
 matrix.case('cli-rust-sessions-unauthorized',bad.returncode not in (0,124) and 'HTTP 401' in bad.stderr and 'Synthetic session' not in bad.stdout,{'exit':bad.returncode})
finally:
 if p.poll() is None:p.send_signal(signal.SIGTERM)
 try:p.wait(timeout=12)
 except subprocess.TimeoutExpired:
  os.killpg(p.pid,signal.SIGKILL);p.wait();raise
 fout.close();ferr.close()
 matrix.case('cli-rust-serve-sigterm-clean',p.returncode==0 and not m.health_ok(addr) and m.process_state(pid,birth)=='exited',{'exit':p.returncode,'rustPid':pid,'childState':m.process_state(pid,birth)})
 matrix.save(binary=binary)
assert all(x['ok'] for x in matrix.cases)
print('minimal real CLI_RUST cases',len(matrix.cases))
