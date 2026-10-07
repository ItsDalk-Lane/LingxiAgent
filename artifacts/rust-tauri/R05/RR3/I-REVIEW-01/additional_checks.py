from review_support import *
import re,shutil,time
s=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text()
old=(ROOT/'artifacts/rust-tauri/R05/RR3/I-01/original-negative-gate.sh').read_text()
section=old[old.index('snapshot_pristine() {'):old.index('\nrecord_case()')]
newprogram=EV/'permanent-final/production-functions.sh'
registry=json.loads((EV/'permanent-final/result.json').read_text())['snapshotRestoreRegistry']
c=EV/'old-independent-copy';p=EV/'old-independent-pristine'
for f in registry:
    dst=c/f;dst.parent.mkdir(parents=True,exist_ok=True);dst.write_bytes((ROOT/f).read_bytes())
p.mkdir()
k='rust/crates/lingxi-kernel/src/lib.rs'
# 使用本轮合法脏候选字节证明恢复没有读取 HEAD。
(c/k).write_bytes((c/k).read_bytes()+'\n// I 独立合法候选\n'.encode())
baseline=sha(c/k)
append=re.search(r"printf '\\n// N06[^\n]+\n\s*>> \"\$COPY/[^\"]+\"[^\n]*",s).group(0)
q=EV/'old-independent-functions.sh'
q.write_text('set -uo pipefail\nCOPY="$1"; PRISTINE="$2"\n'+section+'\n'+append+'\nreset_copy\ncmp "$COPY/'+k+'" "$PRISTINE/'+k+'"\n')
run('old-independent-red',['bash',q,c,p],expected=1)
mutated=sha(c/k);assert mutated!=baseline
run('new-independent-restore-same-pristine',['bash',newprogram,c,p,'reset_copy'],expected=0)
assert sha(c/k)==baseline
save('old-new-independent.json',{'oldSource':str(ROOT/'artifacts/rust-tauri/R05/RR3/I-01/original-negative-gate.sh'),'oldSha256':sha(ROOT/'artifacts/rust-tauri/R05/RR3/I-01/original-negative-gate.sh'),'baseline':baseline,'afterOldReset':mutated,'afterNewReset':sha(c/k),'samePristine':True,'pristineKernelSha256':sha(p/k),'noRecapture':True})
# 原永久回归未包含 mkdir 故障与实际僵尸，这里独立补查。
f=EV/'permanent-final/copy';pr=EV/'permanent-final/pristine'
def shell(name,cmd,expected): return run(name,['bash',newprogram,f,pr,cmd],expected=expected)
mk=EV/'mkdir-fault-pristine'
shell('snapshot-mkdir-failure',f'PRISTINE={mk}; mkdir() {{ echo "mkdir 故障" >&2; return 23; }}; snapshot_pristine {k}',1)
assert not (mk/k).exists()
shell('snapshot-mkdir-recovered',f'PRISTINE={mk}; snapshot_pristine {k}',0)
assert sha(mk/k)==sha(f/k)
log=EV/'permanent-final/sync-production-normal.run.log'
auth=EV/'permanent-final/sync-production-normal-authority.stdout.log'
signal=auth.read_text().strip()
live=subprocess.Popen(['sleep','30'])
try:
    shell('process-query-exit1-nonempty',f'ps() {{ echo S; return 1; }}; n06_gate_running {live.pid}',1)
    shell('process-query-exit2-with-real-log',f'ps() {{ return 2; }}; wait_for_n06_start {live.pid} {log} {__import__("shlex").quote(signal)} 1',1)
    shell('start-log-query-exit2',f'grep() {{ return 2; }}; wait_for_n06_start {live.pid} {log} {__import__("shlex").quote(signal)} 1',1)
    shell('gate-running-positive',f'n06_gate_running {live.pid}',0)
finally:
    live.terminate();live.wait()
# 创建真正已退出但未回收的子进程；按真实 ps 状态等到 Z，随后原函数必须拒绝。
pid=os.fork()
if pid==0: os._exit(0)
try:
    deadline=time.monotonic()+5
    while True:
        query=subprocess.run(['ps','-p',str(pid),'-o','stat='],capture_output=True,text=True)
        if 'Z' in query.stdout: break
        assert time.monotonic()<deadline,'无法观测真实僵尸'
    save('real-zombie-observation.json',{'UTC':utc(),'pid':pid,'argv':['ps','-p',str(pid),'-o','stat='],'exitCode':query.returncode,'stdout':query.stdout,'stderr':query.stderr})
    shell('real-zombie-refused',f'wait_for_n06_start {pid} {log} {__import__("shlex").quote(signal)} 1',1)
finally: os.waitpid(pid,0)
# 权威查询的额外非法形状都在本轮新夹具，保留原字节。
a=f/'rust/crates/xtask/src/stage_maps/R02.json';original=a.read_bytes();doc=json.loads(original)
variants={'empty-scenarios':dict(doc,scenarios=[]),'unknown-command':dict(doc,scenarios=[dict(doc['scenarios'][0],commandRefs=['missing'])]),'non-string-argv':dict(doc,commands=dict(doc['commands']))}
variants['non-string-argv']['commands'][doc['scenarios'][0]['commandRefs'][0]]=dict(doc['commands'][doc['scenarios'][0]['commandRefs'][0]],argv=[123])
try:
    for name,v in variants.items():
        a.write_text(json.dumps(v));shell('authority-'+name,'n06_start_signal /unused || fail "N06 authority query failed"',1)
finally:a.write_bytes(original)
shell('authority-restored','n06_start_signal /unused || fail "N06 authority query failed"',0)
# 原 B 自检与全部独立41控，新输出目录，不修改历史驱动或证据。
run('b15-independent',['python3',ROOT/'scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py'],expected=0)
b41=(ROOT/'artifacts/rust-tauri/R05/RR3/I-01/b41-final-replay.py').read_text()
b41=b41.replace("EV = ROOT / 'artifacts/rust-tauri/R05/RR3/I-01/b41-final'","EV = ROOT / 'artifacts/rust-tauri/R05/RR3/I-REVIEW-01/b41-independent'")
(EV/'b41-independent.py').write_text(b41)
run('b41-independent',['python3',EV/'b41-independent.py'],expected=0)
