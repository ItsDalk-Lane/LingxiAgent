#!/usr/bin/env python3
"""窗口获准后运行独立宿主；普通调用只报告准备态，不启动 Cargo。"""
import argparse,datetime,hashlib,json,os,pathlib,platform,shutil,subprocess,tempfile,time
parser=argparse.ArgumentParser();parser.add_argument('--run',action='store_true');parser.add_argument('--round',default=None);args=parser.parse_args()
base=pathlib.Path(__file__).resolve().parent
repo=base.parents[3]
if not args.run:
 print(json.dumps({'status':'PREPARED_NOT_COMPILED_NOT_EXECUTED','executionRequires':'主集成通知Cargo窗口并明确冻结候选后运行--run','harness':str(base/'harness')},ensure_ascii=False));raise SystemExit(0)
out=base/(args.round or ('run-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')));out.mkdir(exist_ok=False)
fixture=pathlib.Path(tempfile.mkdtemp(prefix='r08-independent-default-')).resolve();home=fixture/'home';tmp=fixture/'tmp';home.mkdir();tmp.mkdir();runtime=fixture/'harness';shutil.copytree(base/'harness',runtime)
env=os.environ.copy();env.update({'LINGXI_HOME':str(home),'TMPDIR':str(tmp),'TMP':str(tmp),'TEMP':str(tmp),'CARGO_TARGET_DIR':str(repo/'rust/target')})
# 全局HOME保持原值；只显式设置本次合成数据根和临时目录。
paths=[]
for crate in ['lingxi-service','lingxi-kernel','lingxi-adapters','lingxi-protocol']:
 p=repo/'rust/crates'/crate;paths.extend(sorted((p/'src').rglob('*.rs')));paths.append(p/'Cargo.toml')
paths.extend([repo/'rust/Cargo.toml',repo/'rust/Cargo.lock',repo/'rust/rust-toolchain.toml'])
paths.extend(sorted((repo/'scripts/rust-tauri').glob('r08-*worker.mjs')))
paths.extend(sorted((repo/'lib/security').glob('*.ts')))
paths.extend([repo/'lib/ptc/ptc-runtime.ts',repo/'lib/pii-guard.ts',repo/'shared/log-redactor.ts'])
def manifest():return {str(p.relative_to(repo)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths if p.is_file()}
runtime_versions={}
for label,cmd in [('rustc',['/Users/study_superior/.cargo/bin/rustup','run','1.98.1','rustc','--version']),('node',['node','--version']),('python3',['python3','--version'])]:
 probe=subprocess.run(cmd,cwd=repo,env=env,text=True,capture_output=True);runtime_versions[label]={'argv':cmd,'exitCode':probe.returncode,'version':probe.stdout.strip(),'path':shutil.which(cmd[0],path=env.get('PATH'))}
before=manifest();command=['/Users/study_superior/.cargo/bin/rustup','run','1.98.1','cargo','test','--manifest-path',str(runtime/'Cargo.toml'),'--offline','--locked','--test','default_plane','default_composition_registry_preparation_and_real_model_tools','--','--exact','--nocapture','--test-threads=1']
start=time.monotonic()
(out/'command.json').write_text(json.dumps({'command':command,'cwd':str(repo),'fixtureRoot':str(fixture),'environment':{k:env[k] for k in ['LINGXI_HOME','TMPDIR','TMP','TEMP','CARGO_TARGET_DIR']},'HOMEUnchanged':env.get('HOME')==os.environ.get('HOME'),'sourceBefore':before},ensure_ascii=False,indent=2)+'\n')
with (out/'output.log').open('wb') as log:
 result=subprocess.run(command,cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT)
after=manifest();changed=sorted(p for p in before.keys()|after.keys() if before.get(p)!=after.get(p))
receipt={'status':'PASS_LOCAL_DEFAULT_COMPOSITION_ONLY' if result.returncode==0 and not changed else 'FAIL_OR_SOURCE_CHANGED','stageAcceptance':'NOT_GRANTED','command':command,'exitCode':result.returncode,'durationSeconds':round(time.monotonic()-start,3),'fixtureRoot':str(fixture),'platform':platform.platform(),'runtimeVersions':runtime_versions,'sourceBefore':before,'sourceAfter':after,'sourceChanges':changed,'outputSha256':hashlib.sha256((out/'output.log').read_bytes()).hexdigest(),'harnessSources':{str(p.relative_to(runtime)):hashlib.sha256(p.read_bytes()).hexdigest() for p in runtime.rglob('*') if p.is_file()},'scope':'五目标默认registry/gateway prepare；默认模型HTTP入口run_code/security_scan/run_tools和真实关库重开；不代领全部原断言/授权矩阵/UI/迁移'}
(out/'receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'receipt':str(out/'receipt.json'),'exitCode':result.returncode,'status':receipt['status']},ensure_ascii=False));raise SystemExit(result.returncode or bool(changed))
