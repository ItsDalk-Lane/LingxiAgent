from pathlib import Path
import json,re,hashlib,shutil,os,platform,subprocess,datetime
OUT=Path(__file__).resolve().parent; MAIN=Path('/Users/study_superior/Desktop/Code/LingxiAgent'); SNAP=OUT/'snapshot'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
text=(OUT/'scope-ancestor-old-red.log').read_text(); roots=set(re.findall(r'/private/var/folders/[^\s"]+/lingxi-xtask-test-rr3-nested-[0-9-]+',text))
for n,root in enumerate(sorted(roots)):
 p=Path(root)/'artifacts'
 if p.exists(): shutil.copytree(p,OUT/f'scope-old-red-artifacts-{n}',dirs_exist_ok=True)
indices=[]
for group in ['nested-internal','nested-external','scope-restored-nested','scope-restored-nested-02','scope-old-red-artifacts-0']:
 for p in sorted((OUT/group).rglob('verify-stage-result.json')):
  j=json.loads(p.read_text()); checkpoints=j['candidateSourceBinding']['checkpointAfterEveryCommand']; cmds=j['commands']; indices.append({'group':group,'path':str(p.relative_to(OUT)),'sha256':sha(p),'overall':j['overall'],'stable':j['candidateSourceBinding']['stable'],'checkpointCount':len(checkpoints),'unstableCount':sum(c['stable']!=True for c in checkpoints),'changedPaths':[bytes.fromhex(s).decode('utf-8','replace') for s in sorted(set(s for c in checkpoints for s in c.get('changedPathBytesHex') or []))],'commandStatuses':[c['status'] for c in cmds]})
(OUT/'checkpoint-index.json').write_text(json.dumps(indices,indent=2)+'\n')
aPaths=['rust/crates/xtask/src/candidate.rs','rust/crates/xtask/src/candidate/tests.rs','rust/crates/xtask/src/main.rs','rust/crates/xtask/src/verify.rs','rust/crates/xtask/src/verify/runner_tests.rs','rust/crates/xtask/src/runner_identity.rs','scripts/rust-tauri/r02_t08_legacy_entry_regression.sh','scripts/rust-tauri/run_output_sinks.py','scripts/rust-tauri/r02_run_output_regression.py','.gitignore','rust-toolchain.toml','rust/Cargo.lock']
aBinding=[{'path':s,'mainSha256':sha(MAIN/s),'snapshotSha256':sha(SNAP/s),'equal':sha(MAIN/s)==sha(SNAP/s)} for s in aPaths]
assert all(j['equal'] for j in aBinding)
main=(SNAP/'rust/crates/xtask/src/main.rs').read_text(); clause='if !stable {\n        // 原有命令或场景失败必须保留；身份漂移只会增加失败原因。\n        report["overall"] = serde_json::Value::String("FAIL".into());'
assert main.count(clause)==1
receipts={'mainHead':subprocess.check_output(['git','rev-parse','HEAD'],cwd=MAIN,text=True).strip(),'mainOverallFrozen':False,'reason':'F46并行；本审查绑定隔离快照A输入，不作为阶段冻结','platform':platform.platform(),'python':platform.python_version(),'toolchainLog':str(OUT/'toolchain.log'),'rustcLog':str(OUT/'rustc.log'),'inputManifestSha256':sha(OUT/'input-manifest.json'),'AInputEquality':aBinding,'mainNotStableForceFail':{'check':'源码审计；原正式cmd_verify_stage独立强制overall FAIL分支仍在','sha256':sha(SNAP/'rust/crates/xtask/src/main.rs'),'occurrences':1,'runtimeBoundary':'Scope旧行为目标红覆盖真实Scope+verify受控编排；不把受控RX映射记正式业务gate'}}
(OUT/'delivery-inputs.json').write_text(json.dumps(receipts,indent=2,ensure_ascii=False)+'\n')
for group in ['nested-internal','nested-external','scope-restored-nested','scope-restored-nested-02']:
 records=[i for i in indices if i['group']==group]; assert sorted(i['checkpointCount'] for i in records)==[7,8,15,20]; assert all(i['stable'] and i['unstableCount']==0 and i['overall']=='PASS' and set(i['commandStatuses'])=={'PASS'} for i in records)
old=[]
for rel in ['artifacts/rust-tauri/R05/RR2/FINAL-01/verify-r05-2/R04_REGRESSION/verify-stage-result.json','artifacts/rust-tauri/R05/RR2/FINAL-01/verify-r05-2/R04_REGRESSION/R03_REGRESSION/verify-stage-result.json']:
 p=MAIN/rel;j=json.loads(p.read_text()); b=j['candidateSourceBinding']; cps=b['checkpointAfterEveryCommand'];old.append({'path':rel,'sha256':sha(p),'overall':j['overall'],'stable':b['stable'],'checkpointCount':len(cps),'unstableCount':sum(c['stable']!=True for c in cps),'changedPaths':sorted(set(bytes.fromhex(s).decode('utf-8','replace') for c in cps for s in c.get('changedPathBytesHex') or []))})
(OUT/'historical-binding-audit.json').write_text(json.dumps(old,indent=2)+'\n')
print('indices',len(indices),'A bytes equal',len(aBinding))
