from capture import *
import shutil

owned=['scripts/rust-tauri/r02_t01_service_smoke.sh','scripts/rust-tauri/r02_t01_log_level_regression.py','rust/crates/lingxi-service/src/redaction.rs','rust/crates/lingxi-service/src/lib.rs']
before=json.loads((EV/'commands/baseline-a01-warn/input-before.json').read_text())
after=snapshot()
changed={k:{'before':before.get(k),'after':after.get(k)} for k in sorted(set(before)|set(after)) if before.get(k)!=after.get(k)}
assert set(changed)-set(owned)=={'scripts/rust-tauri/r05_t08_negative_gate.sh','scripts/rust-tauri/r05_t08_restore_selfcheck.py'},changed
for name in ['rust/crates/lingxi-service/src/logging.rs','rust/crates/lingxi-service/src/main.rs','rust/crates/lingxi-service/src/epoch.rs','rust/crates/lingxi-service/src/inject.rs','rust/crates/lingxi-adapters/src/storage/migrations.rs','rust/Cargo.lock','rust-toolchain.toml']:
    assert before[name]==after[name],name
binary=ROOT/'rust/target/debug/lingxi-service'
expected='3c2c30954478adc179b6b1073d70405a9025722463a132be6ee36c8727ea4865'
assert sha(binary)==expected
for name in ['a01-final-both-levels','a13-final','resources-final','live-correlation-final-command','final-root-build-2']:
    d=json.loads((EV/'commands'/name/'command.json').read_text())
    assert d['exit']==0 and d['binary']['sha256']==expected and d['inputEqual'],name
analysis=json.loads((EV/'RESOURCE_ANALYSIS-resources-final.json').read_text());assert len(analysis['checks'])==20 and all(analysis['checks'].values())
live=json.loads((EV/'live-correlation-final/result.json').read_text());assert live['actual']==5 and live['failed']==0
for row in live['cases']:assert row['assignmentBytes']==47 and row['realMarkerMatch'] and row['realHandledMatch']
negative=json.loads((EV/'sampler-negative-final/result.json').read_text())
assert [r['exitCode'] for r in negative['rows']]==[0,101,101,0]
for row in negative['rows']:
    assert row['targetNamed'] and row['sourceHash']==row['copyHash'] and not row['cleanup']['membersAfter']
    if row['mode']=='normal':
        raw=(EV/'sampler-negative-final'/('normal' if row==negative['rows'][0] else 'restored')/'stdout.log').read_text()
        match=re.search(r'F27 sampler controls: (\{.*\})',raw);assert match
        controls=json.loads(match[1]);assert controls['fdRetained']>=controls['fdBaseline']+3 and controls['tcpRetained']==2 and controls['tcpReleased']==0 and len(controls['filesRetained'])==3 and controls['filesReleased']==[]
for name in ['f47-guard-red','f47-root-red','f48-boundary-red']:
    d=json.loads((EV/'commands'/name/'command.json').read_text());assert d['exit']==1,name
for name in ['f47-restored-green-2','f48-restored-green']:
    d=json.loads((EV/'commands'/name/'command.json').read_text());assert d['exit']==0,name
    mutation=json.loads((EV/(name+'-mutation.json')).read_text());assert mutation['original']==mutation['mutated'],name
for name,counts in [('f46-old-six',[1,2,3,4,4,4]),('f46-restored-six',[1,2,3,3,3,3])]:
    d=json.loads((EV/name/'result.json').read_text());assert [r['count'] for r in d['rows']]==counts and d['cleanup']['allReaped'] and d['cleanup']['homeRemoved']
for name in owned:
    if (EV/'isolated'/name).exists():assert sha(EV/'isolated'/name)==sha(ROOT/name),name
dep=ROOT/'rust/target/debug/lingxi-service.d';shutil.copyfile(dep,EV/'final-root-dependencies.d')
assert '/H-01/isolated/' not in dep.read_text(), '最终程序依赖仍指向副本'
write(EV/'FINAL_SOURCE_BINDING.json',{'UTC':utc(),'state':'SELF_CHECKED','head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'sourceScope':'当前Rust/lock/toolchain/contracts及scripts；不是全仓冻结','ownedFiles':{k:after[k] for k in owned},'allInputHashes':after,'allInputsDigest':hashlib.sha256(json.dumps(after,sort_keys=True).encode()).hexdigest(),'allChangesSinceEntry':changed,'externalChanges':sorted(set(changed)-set(owned)),'binaryPath':str(binary),'binarySha256':sha(binary),'currentResourceEquipmentSha256':sha(ROOT/'rust/target/debug/deps/r05_t08_resources-f66968a3ba499b0b'),'thresholdFileSha256':sha(ROOT/'docs/rust-tauri/PERFORMANCE_THRESHOLDS.json'),'rawResourceSha256':analysis['rawSha256'],'rootDependenciesSha256':sha(dep),'independentReview':'PENDING','stageConclusion':'NOT_ISSUED','R06_READY':'不变；仍未放行'})
rows=[]
for p in sorted((EV/'commands').glob('*/command.json')):
    d=json.loads(p.read_text());label=p.parent.name
    classification='TARGET_RED' if label in ['baseline-a01-warn','baseline-a13','regression-old-red','f47-guard-red','f47-root-red','f48-boundary-red','f46-old-six-command','f47-guard-driver','f47-root-driver','f48-red-driver'] else 'INVALID_PREPARATION_RETAINED' if label in ['f47-restored-green','f47-restore-driver','final-root-build'] else 'EXECUTED'
    counts=[dict(zip(['passed','failed','ignored','measured','filtered'],map(int,m))) for line in d['counts'] for m in re.findall(r'(\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',line)]
    rows.append({'label':label,'classification':classification,'record':str(p.relative_to(EV)),'recordSha256':sha(p),'exit':d['exit'],'RustActualCounts':counts,'leafCases':d['leafCounts'],'nonTestCounts':'不适用；原始输出保留，未把无测试/工具exit0当测试绿' if not counts and not d['leafCounts'] else None})
write(EV/'COMMAND_INDEX.json',rows)
write(EV/'manifest.json',{str(p.relative_to(EV)):{'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(EV.rglob('*')) if p.is_file() and p.name!='manifest.json' and '__pycache__' not in p.parts})
print({'state':'SELF_CHECKED','ownedFiles':len(owned),'commands':len(rows),'actualBinary':sha(binary),'resourceAnalysisChecks':len(analysis['checks']),'independentReview':'PENDING'})
