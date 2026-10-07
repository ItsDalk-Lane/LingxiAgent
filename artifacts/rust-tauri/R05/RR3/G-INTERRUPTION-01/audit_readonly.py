#!/usr/bin/env python3
"""只读取既有证据；仅在本次中断归档目录写摘要，不执行被审脚本。"""
import datetime
import hashlib
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[5]
OUT = pathlib.Path(__file__).resolve().parent
G = ROOT / 'artifacts/rust-tauri/R05/RR3/G-REVIEW-01'
READS = {}

def read(p):
    p = pathlib.Path(p)
    b = p.read_bytes()
    READS[str(p.relative_to(ROOT))] = {'bytes': len(b), 'sha256': hashlib.sha256(b).hexdigest()}
    return b.decode('utf-8', errors='replace')

def js(p):
    return json.loads(read(p))

def sha(p):
    read(p)
    return READS[str(pathlib.Path(p).relative_to(ROOT))]['sha256']

def save(n, d):
    (OUT / n).write_text(json.dumps(d, ensure_ascii=False, indent=2) + '\n')

def counts(t):
    return re.findall(r'test result: [^\n]+', t)

now = datetime.datetime.now(datetime.timezone.utc).isoformat()
authority = ROOT / 'docs/rust-tauri/R05/repair-current'
for n in ['RR1_MASTER_PROMPT_2026-10-04.md', 'RR2_MASTER_PROMPT_2026-10-06.md', 'RR3_BRIEF.md', 'RR3_REVIEW_BRIEF.md', 'RR3_G_BRIEF.md', 'RR3_G_INTERRUPTION_BRIEF.md', 'RR3_HANDOFF.md', 'RR3_ISSUE_MATRIX.json']:
    read(authority / n)
for rel in ['RR2/G-R2/I-MAPPING.md', 'RR2/G-R2/NEG-GATE-RR2.md', 'RR3/A-REVIEW-02/REVIEW.md', 'RR3/B-REVIEW-01/REVIEW.md', 'RR3/C-F46-REVIEW-01/REVIEW.md', 'RR3/D-REVIEW-01/REVIEW.md', 'RR3/E-REVIEW-02/REVIEW.md', 'RR3/I-REVIEW-01/REVIEW.md', 'RR3/H-01/REPORT.md', 'RR3/R02-TRIAGE-01/REPORT.md', 'RR3/TASK0/interruption-recovery-20261007.json']:
    read(ROOT / 'artifacts/rust-tauri/R05' / rel)

events = [json.loads(x) for x in read(G / 'dispatch/events.jsonl').splitlines()]
started, ended = {}, {}
for e in events:
    item = e.get('item', {})
    if item.get('type') == 'command_execution':
        if e['type'] == 'item.started': started[item['id']] = item
        if e['type'] == 'item.completed': ended[item['id']] = item
interruption = {'observedAt': now, 'request': js(G / 'dispatch/request.json'), 'eventCount': len(events), 'eventTypes': sorted(set(e['type'] for e in events)), 'unclosedCommandEvents': [x for k,x in started.items() if k not in ended], 'finalExists': (G / 'dispatch/final.txt').exists(), 'reviewExists': (G / 'REVIEW.md').exists(), 'extraCompleteExists': (G / 'extra-complete.json').exists(), 'workerPermissionEvidenceExists': (G / 'supplements/I06-existing-worker-permission').exists(), 'boundary': '派发外层退出UNKNOWN；另有已落盘子命令回执，不能以事件终止时点抹掉，也不能以子命令退出补造外层完成。'}
save('interruption-index.json', interruption)

default = {'command': js(G / 'default-command.json'), 'logMatches': None, 'rows': [], 'controls': []}
default['logMatches'] = sha(G / 'default-console.log') == default['command']['logSHA256']
for row in read(G / 'default16-01/case-results.tsv').splitlines():
    a = row.split('\t')
    prefix = 'n' + a[0][-2:]
    folder = next(p for p in (G / 'default16-01').iterdir() if p.is_dir() and p.name.startswith(prefix+'-') and p.name != 'n03-restored')
    files = []
    for f in sorted(folder.iterdir()):
        if f.is_file() and (f.suffix == '.log' or 'exit-code' in f.name or f.name=='named-check.txt'):
            text = read(f)
            files.append({'path': str(f.relative_to(G)), 'sha256': sha(f), 'counts': counts(text), 'tail': text[-700:]})
    default['rows'].append({'id': a[0], 'exit': int(a[1]), 'named': a[2], 'verdict': a[3], 'note': a[4], 'files': files})
for n in ['control-xtask', 'control-binwiring', 'n03-restored']:
    q=G/'default16-01'/n
    default['controls'].append({'name':n, 'exit':int(read(q/'exit-code.txt')), 'counts':counts(read(q/'test.stdout.log'))})
for rel in ['n02-zero-match/suites/gaps.txt', 'n04-swallowed-exit/suites/gaps.txt', 'n03-tamper-pin-count/mutation.json']:
    read(G/'default16-01'/rel)
save('default-index.json', default)

supplements=[]
for c in js(G/'supplement-commands.json'):
    q=G/'supplements'/c['name']
    d=js(q/'command.json'); txt=read(q/'stdout.log')
    record={k:v for k,v in d.items() if k not in ['inputBefore','inputAfter']}
    record.update(name=c['name'], logMatches=sha(q/'stdout.log')==d['logSHA256'], inputCountBefore=len(d['inputBefore']), inputCountAfter=len(d['inputAfter']), inputBeforeAfterEqual=d['inputBefore']==d['inputAfter'], countsRecomputed=counts(txt), tail=txt[-1200:])
    supplements.append(record)
save('supplement-index.json', supplements)

roots=['default16-01/n06-midgate-mutation/evidence','default16-01/n16-binding-drift/run-a','default16-01/n16-binding-drift/run-b','supplements/binding-restored-real-R02/evidence','supplements/stable-normal/evidence','supplements/stable-midbyte-red/evidence','supplements/stable-restored/evidence']
results=[]
for rel in roots:
    q=G/rel; d=js(q/'verify-stage-result.json'); b=d['candidateSourceBinding']
    result={'path':rel,'sha256':sha(q/'verify-stage-result.json'),'overall':d['overall'],'stable':b['stable'],'runner':d['runnerSourceBinding']['status'],'checkpointCount':len(b['checkpointAfterEveryCommand']),'stableCheckpointCount':sum(x['stable'] for x in b['checkpointAfterEveryCommand']),'commands':d['commands'],'binding':b}
    if 'stable-' not in rel:
        result['dependencyEvidence']=[]
        for f in ['A05_A06/cli-sessions-owner.stderr.log','CLIENT/cli-help-exit0-no-start/stderr.log','CLIENT/cli-unknown-arg-exit1-no-start/stderr.log','CLIENT/cli-serve-unknown-arg-exit1-no-start/stderr.log','CLI_RUST/serve.stderr.log','CLI_RUST/serve-invalid-bind.stderr.log','CLI_RUST/serve-beta-unavailable.stderr.log','CLI_RUST/serve-downgrade-refused.stderr.log','CLI_RUST/serve-newer-epoch.stderr.log','CLI_RUST/sessions-after-shutdown.stderr.log','CLI_SESSIONS/auth/cli-sessions-owner.stderr.log','a16_legacy_regression/stderr.log']:
            t=read(q/f); result['dependencyEvidence'].append({'path':f,'sha256':sha(q/f),'reasonLines':[x for x in t.splitlines() if 'Cannot find package' in x or 'No such file' in x]})
        result['clientSummary']={k:v for k,v in js(q/'CLIENT/summary.json').items() if k!='sourceSha256'}
        result['clientCases']=[{'case':x['case'],'ok':x['ok'],'exit':x['observed'].get('exitCode')} for x in js(q/'CLIENT/leaf-cases.json')['cases']]
        cr=js(q/'CLI_RUST/cli-rust-cases.json'); result['cliRustCases']=cr['cases'];result['cliRustCommands']=cr['commands']
        result['cliSessionsSummary']={k:v for k,v in js(q/'CLI_SESSIONS/summary.json').items() if k!='sourceSha256'}
        for c in d['commands']:
            if c['status']!='PASS':
                for f in ['stdout.log','stderr.log']: read(q/c['key']/f)
    results.append(result)
save('r02-and-binding-index.json',results)

prod=js(G/'full-producer-restoration-audit.json'); verified=[]
for row in prod['counts']:
    p=G/row['log']; t=read(p)
    verified.append({'path':row['log'],'sha256Matches':sha(p)==row['sha256'],'counts':counts(t)})
series=js(G/'supplements/producer-restored-full/suites/f27-resource-series.json')
save('producer-index.json',{'declared':{k:v for k,v in prod.items() if k!='counts'},'actualLogs':verified,'summary':read(G/'supplements/producer-restored-full/suites/summary.txt'),'seriesKeys':list(series),'seriesSHA256':sha(G/'supplements/producer-restored-full/suites/f27-resource-series.json')})

for name in ['source-drift-default-failure.json','full-restoration.json','copy-restored-inputs.json','supplements-complete.json','extra-registry-mutations.json','mutation-observations.json','n06-binding-audit.json','n16-pair-binding-audit.json','binding-manifest-audit.json','reference-audit-final.json','isolated-copy-reference-equality.json','reuse-a1-targeted-input-audit.json','script-syntax-controls.json','own-resource-analysis.json','environment.json','extra-driver.py','write-review.py']:
    read(G/name)

source_reads=['scripts/rust-tauri/r05_t08_negative_gate.sh','scripts/rust-tauri/r02_t03_auth_matrix.sh','scripts/rust-tauri/r02_client_leaf_matrix.py','scripts/rust-tauri/r02_cli_rust_matrix.py','scripts/rust-tauri/r02_cli_sessions_leaf_matrix.py','scripts/rust-tauri/r02_t08_legacy_entry_regression.sh','cli/client.ts','package.json','package-lock.json','.gitignore']
for n in source_reads: read(ROOT/n)
save('raw-reference-digests.json',{'observedAt':now,'method':'完整读取所列原文件并计算SHA256；不执行其中脚本、不重跑Cargo、不访问副本或缓存。只读归档不代替新G02亲跑。','sources':READS})
print(json.dumps({'defaultRows':len(default['rows']),'defaultLogMatches':default['logMatches'],'completedSupplementReceipts':len(supplements),'allSupplementLogsMatch':all(x['logMatches'] for x in supplements),'allProducerLogsMatch':all(x['sha256Matches'] for x in verified),'producerLogs':len(verified),'rawReferenceFiles':len(READS),'unclosedEvents':[x['id'] for x in interruption['unclosedCommandEvents']],'workerPermissionEvidenceExists':interruption['workerPermissionEvidenceExists']},ensure_ascii=False))
