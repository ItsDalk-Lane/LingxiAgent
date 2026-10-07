from pathlib import Path
import json,hashlib
ROOT=Path.cwd();E=ROOT/'artifacts/rust-tauri/R05/RR3/E-01';D=ROOT/'docs/rust-tauri/R05'
def sha(p):return hashlib.sha256((ROOT/p).read_bytes()).hexdigest()
base='artifacts/rust-tauri/R05/RR2/FINAL-01/verify-r05-2/R04_REGRESSION/R03_REGRESSION/'
c=json.loads((E/'current-state.json').read_text());refs={'rr2_r02_summary':base+'R02/A16/legacy-entry/summary.txt','rr2_r02_stdout':base+'r02_legacy_regression/stdout.log','rr2_r02_exclusions':base+'R02/A16/legacy-entry/e0-binding-exclusions.txt','rr2_r02_sinks':base+'R02/A16/legacy-entry/e0-run-output-sinks.txt'};c['evidence_refs'].update(refs)
c['historical_r02']={'command':'r02_legacy_regression','parent_result':base+'verify-stage-result.json','status':'PASS','exit_code':0,'mode':'directed-no-seal-family','E0_to_E4_5':'ALL GREEN','E5':'SKIP BY SCOPE（原明确许可，非完整npm或正式封印绿）','summary_ref':refs['rr2_r02_summary'],'raw_stdout_ref':refs['rr2_r02_stdout'],'scope':'历史RR2 FINAL第4层实际R02脚本结果，不冒称独立R02 verify-stage JSON；其上游R03/R04来源不稳定仍FAIL，不能推导全链PASS。','candidate_sha':'ad5ec4e9853a51ed929f1e2e077b97d41c951572'}
(E/'current-state.json').write_text(json.dumps(c,ensure_ascii=False,indent=2)+'\n')
for name in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json']:
 p=D/name;d=json.loads(p.read_text());old_path=E/'before/docs/rust-tauri/R05'/name;old=json.loads(old_path.read_text());extra={k:v for k,v in d.items() if k not in old};extra['rr3_current']=c;s=old_path.read_text();i=s.rfind('}');p.write_text(s[:i].rstrip()+',\n'+json.dumps(extra,ensure_ascii=False,indent=2)[2:-2]+'\n}\n')
p=D/'R05_HANDOFF.json';h=json.loads(p.read_text());h['rr3_current']=c;h['artifact_hashes'].update({p:sha(p) for p in refs.values()});p.write_text(json.dumps(h,ensure_ascii=False,indent=2)+'\n')
p=ROOT/'docs/rust-tauri/ORCHESTRATOR_PROGRESS.json';o=json.loads(p.read_text());o['stages']['R05']['rr3_current']=c;p.write_text(json.dumps(o,ensure_ascii=False,indent=2)+'\n')
p=D/'R05_REPORT.md';s=p.read_text();needle='### 11.2 资源、自检与普通取消的边界';s=s.replace(needle,'第四层R02实际由R03的 `r02_legacy_regression` 命令执行，exit0、PASS：'+f'[原始summary](../../../{refs["rr2_r02_summary"]})'+'记录E0–E4.5 ALL GREEN与E5 SKIP BY SCOPE（`directed-no-seal-family`原明确许可）。这是R02脚本历史结果，不冒称有独立R02 verify-stage JSON；R03/R04来源漂移与全链FAIL仍成立，不把E5跳过说成raw npm全绿。\n\n'+needle);p.write_text(s)
p=E/'verify.py';s=p.read_text();needle="rr=obj(c['evidence_refs']['resource_receipt']);";insert="""r02=c['historical_r02'];parent=obj(r02['parent_result']);command=next(r for r in parent['commands'] if r['key']=='r02_legacy_regression')
 check(command['status']=='PASS' and command['exitCode']==0 and 'R02_LEGACY_REGRESSION_MODE=directed-no-seal-family' in command['argv'],'real fourth layer R02 directed command')
 text=read(r02['summary_ref']);check('SKIP E5 (full npm + seal-family classification) BY SCOPE' in text and 'RESULT: R02 legacy entry regression DIRECTED (E0–E4.5) ALL GREEN' in text,'R02 real summary keeps E5 scope')
 """;s=s.replace(needle,insert+needle);p.write_text(s)
p=E/'run-checks.py';s=p.read_text().replace("checks-02'","checks-03'").replace("commands-02.json","commands-03.json");p.write_text(s)
p=E/'finish.py';s=p.read_text().replace('commands-02.json','commands-03.json').replace('`checks-02/`','`checks-03/`').replace('原始证据44项核对','原始证据逐项核对').replace('RR2 FINAL正式结果：R05','RR2 FINAL第四层R02实际脚本r02_legacy_regression exit0，E0–E4.5 ALL GREEN，E5 SKIP BY SCOPE（directed-no-seal-family）；不是独立R02 verify-stage JSON，原始summary/stdout/exclusions/sinks哈希已纳入HANDOFF。RR2 FINAL正式结果：R05');p.write_text(s)
print('Fourth-layer R02 actual command and raw scoped evidence added; no new result invented')
