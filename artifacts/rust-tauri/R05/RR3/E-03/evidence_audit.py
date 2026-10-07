import json, pathlib
from audit import ROOT, EV, G, H, load, save, sha, entry, now

results=[]
def check(name,ok):
    results.append(dict(name=name,passed=bool(ok)))
    assert ok,name
run=load(G/'metadata/run-index.json'); current=load(EV/'current-state.json')['packages']['G']
check('default shell raw exit is unknown',run['defaultRawExit']==current['default_shell_exit']=='UNKNOWN')
check('recorder and observer distinct',run['recorderExit']==1 and run['observerExit']==143)
check('actual default scope complete intended but not completed',run['commandStart']['argv']==['bash','scripts/rust-tauri/r05_t08_negative_gate.sh','artifacts/rust-tauri/R05/RR3/G-REVIEW-02/default16-01'] and run['commandStart']['environment']['R02_LEGACY_REGRESSION_MODE'] is None)
check('target rows exact',run['completedValidNegativeTargets']==['N01'] and run['invalidNegativeTargets']==['N02'] and run['notExecuted']==[f'N{x:02d}' for x in range(3,17)])
check('tsv original two rows',len(run['recordedCaseRows'])==2 and (G/'default16-01/case-results.tsv').read_text().splitlines()==run['recordedCaseRows'])
logs=[]
for record in run['records']:
    if 'log' in record:
        p=pathlib.Path(record['log']);p=p if p.is_absolute() else ROOT/p
        if not p.exists():p=G/record['log']
        check('G actual log '+record['name'],sha(p.read_bytes())==record['sha256'])
        logs.append(entry(p))
    elif 'buildLog' in record:
        value=record['buildLog'];p=ROOT/value if isinstance(value,str) else ROOT/value['path']
        if not p.exists():p=G/(value if isinstance(value,str) else value['path'])
        check('N02 real ENOSPC','No space left on device' in p.read_text(errors='replace'))
        check('N02 target not reached',record['targetReached'] is False and record['runtimeSuitesCompleted']==0)
        logs.append(entry(p))
check('full R02 and E5 absent',run['fullR02']=='NOT RUN' and run['fullE5']=='NOT RUN')
stop=load(G/'STOPPED.json');check('G writers stopped',stop['activeCargoRustcOrG02']==[] and stop['knownProcessesPresent']==[] and stop['noMoreBuilds'])

for label in ['resources-final','ordinary-cancel-recovery','late-result-next-run','redaction-final-2','logging-final']:
    p=H/'commands'/label;record=load(p/'command.json')
    check('H actual output '+label,sha((p/'stdout.log').read_bytes())==record['stdoutSHA256'] and sha((p/'stderr.log').read_bytes())==record['stderrSHA256'])
    logs.extend([entry(p/'command.json'),entry(p/'stdout.log'),entry(p/'stderr.log')])

old=load(ROOT/'artifacts/rust-tauri/R05/RR3/I-01/manifest.json');historical=load(ROOT/'artifacts/rust-tauri/R05/RR3/I-REVIEW-01/history-audit.json');diff=[]
for path in historical['differences']:
    file=ROOT/'artifacts/rust-tauri/R05/RR3/I-01'/path;actual=entry(file);diff.append(dict(path=path,old_manifest=old['files'][path],current_sha256=actual['sha256'],current_bytes=actual['bytes'],equal=old['files'][path]['sha256']==actual['sha256']))
save('historical-dispatch-differences.json',dict(at=now(),historical_review_ref='artifacts/rust-tauri/R05/RR3/I-REVIEW-01/history-audit.json',historical_review_sha256=sha((ROOT/'artifacts/rust-tauri/R05/RR3/I-REVIEW-01/history-audit.json').read_bytes()),historical_checked=historical['checked'],original_manifest_count=historical['manifestFileCount'],differences=diff,scope='Only the three previously recorded dynamic dispatch differences were newly rehashed. The previous independent 8281 unchanged non-dispatch files are a historical statement, not a new complete manifest audit. Original manifest unmodified.'))
check('dynamic manifest differences preserved',len(diff)==3 and all(not x['equal'] for x in diff))

authority=load(G/'metadata/authority-files.json')['files'];reading=[]
for row in authority:
    p=ROOT/row['path']
    if 'specifications/' in row['path'] or 'MASTER_PROMPT' in row['path']:
        data=p.read_bytes();check('authority original '+row['path'],sha(data)==row['sha256']);reading.append(dict(path=row['path'],sha256=sha(data),bytes=len(data),scope='Original 10-02 + eight taskbooks fully read by read-only doc_input_audit; R06 consumption/precondition only used; RR1/RR2 Masters fully read by E03 and relevant clauses checked again. Byte identity checked here.'))
save('authority-reading.json',dict(at=now(),main_actor='/root/rr3_e_impl_03',read_only_helper='/root/rr3_e_impl_03/doc_input_audit',independent_review=False,files=reading,scope_preserved=['16A','100 original + 3 additional C','130 applicable leaves','I01–I11','N01–N16','RR1 §6.1/§6.2','four original platform groups','no R06 work']))
save('evidence-self-check.json',dict(at=now(),status='PASS',checks=results,raw_logs=logs,executed_product_tests=False))
print('evidence source checks',len(results),'PASS; historical three dynamic changes retained')
