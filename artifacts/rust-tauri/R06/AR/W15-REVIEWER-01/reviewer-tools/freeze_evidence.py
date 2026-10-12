import pathlib,json,hashlib,subprocess,datetime,os,re,ctypes,time
ROOT=pathlib.Path('/private/tmp/lingxi-r06-w15-reviewer-01');OUT=ROOT/'artifacts/rust-tauri/R06/AR/W15-REVIEWER-01';BASE=pathlib.Path('/private/tmp/lingxi-w15-r01-runtime')
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def load(p):return json.loads((OUT/p).read_text())
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def save(p,v):(OUT/p).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
began=now();monotonic=time.monotonic();audit=load('PROCESS_SAMPLING_AUDIT.json');known={x['pid'] for x in audit['records']};selfpid=os.getpid()
lib=ctypes.CDLL('/usr/lib/libproc.dylib');lib.proc_pidpath.argtypes=[ctypes.c_int,ctypes.c_void_p,ctypes.c_uint32]
def executable(pid):
 b=ctypes.create_string_buffer(4096);return b.value.decode() if lib.proc_pidpath(pid,b,len(b))>0 else None
relevant=[];survivors=[];pid_reuse=[]
for line in subprocess.check_output(['ps','-axo','pid=,ppid=,rss=,stat=,args='],text=True).splitlines():
 a=line.strip().split(None,4)
 if len(a)!=5:continue
 pid=int(a[0]);path=executable(pid);args=a[4];ownedpath=bool(path and (path.startswith(str(BASE)) or path.startswith(str(ROOT))))
 businessarg=bool(re.search(r'^\S*(?:python[^ ]*|cargo|rustc|xtask|lingxi-service|r06_[^ ]*)\s',args) and (str(BASE) in args or str(ROOT) in args))
 if pid in known or ownedpath or businessarg or pid==selfpid:
  row=dict(pid=pid,ppid=int(a[1]),rssBytes=int(a[2])*1024,stat=a[3],actualExecutable=path,argv=args,ownedByRuntimePath=ownedpath,selfMetadataWriter=pid==selfpid)
  relevant.append(row)
  if pid==selfpid:continue
  if ownedpath or businessarg:survivors.append(row)
  elif pid in known:pid_reuse.append(dict(pid=pid,currentExecutable=path,classification='REUSED_PID_NOT_OWNED_DO_NOT_SIGNAL'))
assert not survivors, survivors
be=OUT/'windows/independent-be-battery-02/raw/facts.jsonl';facts=[json.loads(x) for x in be.read_text().splitlines()];ready=[x['value'] for x in facts if x['kind']=='service_ready'];exitrows=[x['value'] for x in facts if x['kind']=='service_exit']
assert {x['pid'] for x in ready}=={x['pid'] for x in exitrows} and all(x['exitCode']==0 for x in exitrows)
standalone=[]
for win,filename in [('original-persona-next-run','persona-next-run-observations.json'),('original-resource-precondition','resource-production-observations.json')]:
 d=load('windows/'+win+'/'+filename);standalone.append(dict(window=win,pid=d['pid'],ready=d['ready'],exitCode=d['serviceExitCode'],binaryBefore=d['binarySha256Before'],binaryAfter=d['binarySha256After'],kind='PROBE_DIRECT_POPEN_WAIT'))
collision=load('windows/independent-real-home-collision-02/collision.json');standalone += [dict(window='independent-real-home-collision-02',pid=x['pid'],ready=x['ready'],exitCode=x['serviceExitCode'],binaryBefore=collision['binaryBefore'],binaryAfter=collision['binaryAfter'],kind='PROBE_DIRECT_POPEN_WAIT') for x in collision['worlds']]
assert all(x['exitCode']==0 and x['binaryBefore']==x['binaryAfter'] for x in standalone)
save('PID_CLOSURE.json',dict(checkedAt=now(),allBusinessBuildCargoServiceAndProbeControllersClosed=True,knownProcessRecords=len(audit['records']),distinctKnownPids=len(known),businessSurvivors=survivors,finalRelevantPsRows=relevant,pidReuse=pid_reuse,perWindowClosure='WINDOW_INDEX.json plus each windows/*/run.json',allGuardBreaches=[],beOriginalReady=ready,beActualServiceExit=exitrows,otherActualServiceReadyExit=standalone,scope28='run.json descendant observed absent; no separate exit code fabricated',invalidCollision='original failed-precondition probe stdout/exception and actual guard closure retained',waitSemantics='Reviewer Popen.wait only direct controller; child services waited by original test/probe; sampled descendant absence not independently waitable; short-lived/unseen measurements unknown/null, not zero.',excludedActiveMetadataWriter=dict(pid=selfpid,argv='freeze_evidence.py metadata-only writer; no business subprocess; direct shell tool returns actual exit separately')))
original_inputs=json.loads((ROOT/'artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01/frozen-inputs-manifest.json').read_text());transfer=json.loads((ROOT/'artifacts/rust-tauri/R06/AR/W15-REVIEW-PREP-01/readonly-input-provenance-transfer.json').read_text());relocated={x['path']:x['snapshot_path'] for x in transfer['historical_mutable_inputs']};readback=[]
for x in original_inputs:
 p=relocated.get(x['path'],x['path']);actual=sha(ROOT/p)
 if actual!=x['sha256']:
  assert x['path']=='docs/rust-tauri/R06/repair-current/dispatches/R06-AR-W14_EXECUTOR_01.md'
  transfer_source=json.loads((ROOT/'artifacts/rust-tauri/R06/AR/W15-REVIEW-PREP-01/exact-frozen-candidate-transfer.json').read_text())['author_workspace']
  original=pathlib.Path(transfer_source)/x['path'];assert sha(original)==x['sha256']
  target=BASE/'readonly-historical-inputs'/x['path'];target.parent.mkdir(parents=True,exist_ok=True)
  if target.exists():assert sha(target)==x['sha256']
  else:target.write_bytes(original.read_bytes())
  readback.append(dict(originalPath=x['path'],actualPath=str(target),originalSha=x['sha256'],actualSha=sha(target),currentControlSha=actual,classification='FIFTH_HISTORICAL_MUTABLE_DIFFERENCE_LATE_READONLY_RECOVERY_FROM_AUTHOR_CURRENT_CONTROL_PRESERVED',actualRecoveredAt=now(),actualSource=str(original),notBehaviorInputSwap=True))
 else:readback.append(dict(originalPath=x['path'],actualPath=p,originalSha=x['sha256'],actualSha=actual,classification='HISTORICAL_MUTABLE_SNAPSHOT_NOT_CURRENT_CONTROLS' if p!=x['path'] else 'EXACT_ORIGINAL_READONLY'))
assert len(readback)==269
save('ORIGINAL_INPUT_PROVENANCE_FINAL.json',dict(inputs=readback,count=269,currentControls='frozen-input-manifest.json verifies all 116 current separately',allOriginalBytesExactReadback=True,currentControlsUnchangedFromReviewerStart=True,fifthAuthorHistoricalDispatchRecoveredLateWithoutCurrentControlOverwrite=True))
runtime=[]
for p in sorted(BASE.rglob('*')):
 if not p.is_file() or p.is_symlink() or (BASE/'target') in p.parents:continue
 runtime.append(dict(path=str(p),bytes=p.stat().st_size,sha256=sha(p),classification='LOCAL_ONLY_OWN_SYNTHETIC_INPUT_HOME_DATABASE_CONFIG_PRODUCT_BYTES'))
bins={x['actual_executable']:x['executable_sha256_after'] for x in audit['records'] if x['actual_executable'] and x['actual_executable'].startswith(str(BASE)) and x['executable_sha256_after']}
for p,h in bins.items():assert sha(p)==h
save('LOCAL_ONLY_INPUTS.json',dict(runtimeRoot=str(BASE),cloneRoot=str(ROOT),noFullCloneOrTargetArchive=True,sourceOriginalFiles='frozen-source-manifest.json SHA-only; original bytes LOCAL_ONLY',runtimeFiles=runtime,executedProductBinaries=[dict(path=p,sha256=h,classification='LOCAL_ONLY_ACTUAL_EXECUTED_BINARY') for p,h in sorted(bins.items())],targetCache='LOCAL_ONLY_EXCLUDED_FROM_TRANSFER_NO_INVENTORY_CLAIM',note='首次无效collision config0被自己的修正窗口覆盖，原配置字节缺失限制见PREPARATION_AND_EVIDENCE_LIMITS；实际后续合成输入最终SHA仅绑定本地当前原物。'))
save('TOOLCHAIN_AND_ENV_FINAL.json',dict(cargoCommand='/Users/study_superior/.cargo/bin/cargo',actualVersion=subprocess.check_output(['/Users/study_superior/.cargo/bin/cargo','--version'],text=True).strip(),lockSha256=sha(ROOT/'rust/Cargo.lock'),actualCargoExecutableSha256=[dict(path=x['actual_executable'],sha256=x['executable_sha256_after']) for x in audit['records'] if x['actual_executable'] and pathlib.Path(x['actual_executable']).name=='cargo'][:1],perWindowEffectiveEnvironment='WINDOW_INDEX.json',osHomeInherited='/Users/study_superior',productDir=str(ROOT/'lib'),externalHomeTmpTarget=True,incremental='0',allActualGuardBreaches=[],sampledPeakRssBytes=audit['maxSampledRssBytes'],unknownExecutablePaths=audit['unknownExecutablePath'],unknownRssOnSampledRecords=audit['unknownRss'],measurementLimit='unobserved transient processes unknown; sampledRSS not absolute peak'))
source=load('frozen-source-manifest.json');assert all(sha(ROOT/x['path'])==x['sha256'] for x in source['files'])
manifest=[]
for p in sorted(OUT.rglob('*')):
 if not p.is_file() or p.is_symlink() or p.name in ('safe-evidence-manifest.json','END.json'):continue
 assert p.suffix in ('.json','.jsonl','.log','.md','.py','.stderr'),p
 manifest.append(dict(path=str(p.relative_to(OUT)),sha256=sha(p),bytes=p.stat().st_size,transfer='SAFE_SYNTHETIC_EVIDENCE_OR_SHA_ONLY'))
save('safe-evidence-manifest.json',dict(root=str(OUT),files=manifest,fileCount=len(manifest),totalBytes=sum(x['bytes'] for x in manifest),excluded=['END.json detached closure envelope','safe-evidence-manifest.json self','all actual source/input/db/home/config/bin/target/clone bytes outside evidence root'],noRealUserOrPaidProviderData=True,noFullSourceArchive=True))
assert all(sha(OUT/x['path'])==x['sha256'] for x in manifest)
save('END.json',dict(at=now(),actor='/root/ar_w15_reviewer_01',role='FIRST_FRESH_INDEPENDENT_REVIEW',verdict='FAIL',businessResourcesClosed=True,actorEndedOneTimeNoReuse=True,sourceCount=4059,sourceManifestSha256=sha(OUT/'frozen-source-manifest.json'),inputManifestSha256=sha(OUT/'frozen-input-manifest.json'),originalInputProvenanceSha256=sha(OUT/'ORIGINAL_INPUT_PROVENANCE_FINAL.json'),safeManifestSha256=sha(OUT/'safe-evidence-manifest.json'),safeFiles=len(manifest),safeBytes=sum(x['bytes'] for x in manifest),reportSha256=sha(OUT/'REPORT.md'),resultsSha256=sha(OUT/'RESULTS.json'),pidClosureSha256=sha(OUT/'PID_CLOSURE.json'),candidate=load('START_BINDING.json')['candidate'],fullStage='NOT_RUN_NOT_STAGE_ROLE',commitPush='NONE',guardBreaches=[],businessSurvivors=[],samplingUnknownPaths=audit['unknownExecutablePath'],metadataWriter=dict(pid=selfpid,started=began,recordWritten=now(),elapsedAtWrite=time.monotonic()-monotonic,actualExit='Tool-call actual completion will be read after this writer returns; this field is not fabricated exit0')))
print(json.dumps(dict(verdict='FAIL',safeFiles=len(manifest),safeBytes=sum(x['bytes'] for x in manifest),safeManifestSha256=sha(OUT/'safe-evidence-manifest.json'),sourceManifestSha256=sha(OUT/'frozen-source-manifest.json'),inputManifestSha256=sha(OUT/'frozen-input-manifest.json'),pidClosureSha256=sha(OUT/'PID_CLOSURE.json'),endSha256=sha(OUT/'END.json'),businessSurvivors=[],originalInputs269Unchanged=True)))
