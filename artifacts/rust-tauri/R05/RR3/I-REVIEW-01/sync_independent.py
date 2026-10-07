from review_support import *
import re,shlex,time
f=EV/'permanent-final';c=f/'runner-copy';p=f/'runner-pristine';program=f/'production-functions.sh';binary=f/'own-target/debug/xtask'
s=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text();registry=json.loads((f/'result.json').read_text())['snapshotRestoreRegistry']
authority=c/'rust/crates/xtask/src/stage_maps/R02.json';mapdoc=json.loads(authority.read_text());key=mapdoc['scenarios'][0]['commandRefs'][0];release=Path(mapdoc['commands'][key]['argv'][-1]);k='rust/crates/lingxi-kernel/src/lib.rs'
base={x:sha(c/x) for x in registry};binarysha=sha(binary);rows=[]
outbase=c/'artifacts/f49-independent';outbase.mkdir(parents=True,exist_ok=False)
def shell(name,cmd,expected=0): return run(name,['bash',program,c,p,cmd],cwd=c,expected=expected)
for name,mutate in [('normal',False),('midbyte',True),('restored',False)]:
    release.unlink(missing_ok=True);evidence=outbase/name;log=outbase/(name+'.live.log');argv=[str(binary),'verify-stage','R02','--evidence',str(evidence)]
    started=utc();sourcebefore={x:sha(c/x) for x in registry}
    with log.open('w') as out:
        gate=subprocess.Popen(argv,cwd=c,stdout=out,stderr=subprocess.STDOUT)
        try:
            shell('own-sync-'+name+'-authority',f'n06_start_signal {shlex.quote(str(evidence))}')
            signal=(EV/('own-sync-'+name+'-authority.stdout.log')).read_text().strip()
            waitreceipt=shell('own-sync-'+name+'-wait',f'wait_for_n06_start {gate.pid} {shlex.quote(str(log))} {shlex.quote(signal)} 10')
            captured=utc();actual=log.read_text();assert signal in actual.splitlines();assert gate.poll() is None
            process=subprocess.run(['ps','-p',str(gate.pid),'-o','stat='],capture_output=True,text=True);assert process.returncode==0 and process.stdout.strip() and 'Z' not in process.stdout
            shell('own-sync-'+name+'-live-check',f'n06_gate_running {gate.pid}')
            appended=None
            if mutate:
                app=re.search(r"printf '\\n// N06[^\n]+\n\s*>> \"\$COPY/[^\"]+\"[^\n]*",s).group(0)
                appended=shell('own-sync-'+name+'-append',app)
            release.write_text('释放当前真实控制命令\n');code=gate.wait(timeout=20)
        finally:
            release.write_text('结束当前控制命令\n')
            if gate.poll() is None:gate.terminate();gate.wait(timeout=20)
    ended=utc();r=json.loads((evidence/'verify-stage-result.json').read_text());binding=r['candidateSourceBinding'];expected=not mutate
    assert code==(0 if expected else 1) and r['overall']==('PASS' if expected else 'FAIL') and binding['stable']==expected
    assert len(r['commands'])==1 and all(x['exitCode']==0 and x['status']=='PASS' for x in r['commands'])
    assert r['runnerSourceBinding']['status']=='PASS' and len(binding['checkpointAfterEveryCommand'])==1
    beforems=binding['before']['atUnixMs'];assert beforems < datetime.datetime.fromisoformat(waitreceipt['endUTC']).timestamp()*1000
    if appended:assert waitreceipt['endUTC']<=captured<=appended['startUTC']
    excluded=binding['excluded']['runOutputFiles'];assert any(x['relativePath']==str(log.relative_to(c)) and x['kind']=='FILE' for x in excluded),excluded
    changed=[bytes.fromhex(x).decode() for x in binding['finalChangedPathBytesHex']];assert changed==([k] if mutate else [])
    sourceafter={x:sha(c/x) for x in registry};assert [x for x in base if sourcebefore[x]!=sourceafter[x]]==([k] if mutate else [])
    row={'name':name,'argv':argv,'cwd':str(c),'startUTC':started,'endUTC':ended,'exitCode':code,'expectedExitCode':0 if expected else 1,'pidAtCapture':gate.pid,'realPS':{'argv':['ps','-p',str(gate.pid),'-o','stat='],'exitCode':process.returncode,'stdout':process.stdout,'stderr':process.stderr},'beforeAtUnixMs':beforems,'waitEndUTC':waitreceipt['endUTC'],'capturedSignalUTC':captured,'expectedSignal':signal,'capturedSignal':signal,'appendStartUTC':appended['startUTC'] if appended else None,'sourceBefore':sourcebefore,'sourceAfter':sourceafter,'overall':r['overall'],'stable':binding['stable'],'commandsAllGreen':True,'checkpointCount':1,'changedPaths':changed,'realFDExclusions':excluded,'binarySha256':sha(binary),'authoritySha256':sha(authority),'resultSha256':sha(evidence/'verify-stage-result.json'),'logSha256':sha(log)}
    with (EV/'commands.jsonl').open('a') as out:out.write(json.dumps(row,ensure_ascii=False)+'\n')
    shell('own-sync-'+name+'-restore','reset_copy');assert {x:sha(c/x) for x in registry}==base and sha(binary)==binarysha
    rows.append(row)
# 最后一项真实 N16 追加语句，然后调用原终末 reset；同一 pristine 精确恢复。
n16=re.search(r"printf '\\n// N16[^\n]+\n\s*>> \"\$COPY/[^\"]+\"[^\n]*",s).group(0)
shell('own-final-N16-append',n16);assert sha(c/k)!=base[k]
shell('own-final-N16-reset','reset_copy');assert {x:sha(c/x) for x in registry}==base
save('sync-independent-audit.json',{'status':'PASS','rows':rows,'finalN16ExactRestore':True,'samePristine':True,'boundary':'真实生产main/verify/Scope/Git/OS fd，新独立RX控制map只证明机制，不冒充完整R02业务或16负测；未手传FILE'})
