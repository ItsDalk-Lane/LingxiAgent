import sys,json,subprocess,time,hashlib,os,tempfile,re
from pathlib import Path
BASE=Path(__file__).resolve().parent
OWNED=['rust/crates/lingxi-service/src/runs.rs','rust/crates/lingxi-service/src/hooks.rs','rust/crates/lingxi-kernel/src/memory.rs','rust/crates/lingxi-service/tests/r06_ar_f08_projection.rs']
def hashes(): return {p:hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in OWNED}
tag=sys.argv[1]; cmd=sys.argv[2:]; env=os.environ.copy();home=tempfile.mkdtemp(prefix='lingxi-w08-'+tag+'-');env.update(LINGXI_HOME=home,TEST_GUARD_VERBOSE='1',TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',LINGXI_TEST_GUARD='1',LINGXI_F08_EVIDENCE=str(BASE/'observations'/tag))
start=time.time();before=hashes();log=BASE/'logs'/f'{tag}.log'
assert not log.exists(),'不得覆盖先前尝试日志'
with log.open('w') as out:
 out.write(json.dumps({'command':cmd,'start':start,'LINGXI_HOME':home,'guard':'8GiB/600s','owned':before},ensure_ascii=False)+'\n');out.flush();result=subprocess.run(cmd,env=env,stdout=out,stderr=subprocess.STDOUT)
text=log.read_text();counts=[int(m) for m in re.findall(r'running (\d+) tests?',text)]
record={'tag':tag,'candidate_sha':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'owned_before':before,'owned_after':hashes(),'toolchain':'cargo1.98.1','lock_hash':hashlib.sha256(Path('rust/Cargo.lock').read_bytes()).hexdigest(),'fixture_hash':before[OWNED[-1]],'platform':subprocess.check_output(['uname','-a'],text=True).strip(),'command':cmd,'start_time':start,'end_time':time.time(),'exit_code':result.returncode,'test_count':sum(counts),'LINGXI_HOME':home,'evidence_sha256':hashlib.sha256(log.read_bytes()).hexdigest(),'mock_boundary':'仅loopback模型/外部工具多块应答；生产guard/消息组装/网关/存储不mock','cwd':str(Path.cwd()),'CARGO_TARGET_DIR':str(Path(env.get('CARGO_TARGET_DIR','rust/target')).resolve()),'status':'PASS' if result.returncode==0 else 'FAIL'}
(BASE/f'{tag}.json').write_text(json.dumps(record,ensure_ascii=False,indent=2));print(json.dumps({'tag':tag,'exit_code':result.returncode,'test_count':sum(counts)}));sys.exit(result.returncode)
