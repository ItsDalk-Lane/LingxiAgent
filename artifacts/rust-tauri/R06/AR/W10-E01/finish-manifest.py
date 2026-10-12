from pathlib import Path
import json,hashlib,datetime,subprocess,os,re
root=Path(__file__).resolve().parents[5];ev=Path(__file__).resolve().parent;sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
# 清单准备失败的原收据保留，不覆盖原始业务、门禁或失败日志。
(ev/'evidence-manifest.json').rename(ev/'evidence-manifest-preparation-01.json')
(ev/'final-verification.json').rename(ev/'final-verification-preparation-01.json')
failed={'at':now(),'cwd':str(root),'argv':['python3','-'],'exit':1,'classification':'HANDOFF_METADATA_PREPARATION_FAILURE_NOT_BUSINESS_RED','reason':'Single JSON parser met original 7-line JSONL key-events-dump.json; subsequent scan found intentional empty negative backup stdout s3-backup-out.json. Original records unchanged.','source_owned_unchanged':True,'product_changed':False,'source_input_before_start_not_separately_attested':True,'raw_failure':'json.decoder.JSONDecodeError: Extra data: line 2 column 1 (char 337)','retained':['evidence-manifest-preparation-01.json','final-verification-preparation-01.json']}
(ev/'manifest-preparation-01.json').write_text(json.dumps(failed,ensure_ascii=False,indent=2)+'\n')
f=ev/'REPORT.md';t=f.read_text();t+='\n清单准备首次把所有.json当单记录解析，遇到原7行JSONL及负向用例故意空stdout而失败；`manifest-preparation-01.json` 与两份准备快照保留，最终清单按真实格式登记。未改写原始业务/阶段记录，不能将此准备失败计为产品红。\n';f.write_text(t)
r=json.loads((ev/'final-verification-preparation-01.json').read_text());r['at']=now();r['all_json_evidence_parseable']=False;r['format_exceptions']=[{'path':'verify-R05-final-01/R04_REGRESSION/R03_REGRESSION/R02/A09_A10/key-events-dump.json','format':'JSONL','valid_records':7},{'path':'verify-R05-final-01/R04_REGRESSION/R03_REGRESSION/R02/A11/backup-restore/s3-backup-out.json','format':'INTENTIONALLY_EMPTY_NEGATIVE_STDOUT','valid_single_json':False}];r['other_json_evidence_parseable']=True
freeze=json.loads((ev/'owned-freeze-before-formal.json').read_text());assert all(sha(root/x['path'])==x['sha256'] for x in freeze['owned']);assert sha(root/'rust/Cargo.lock')==freeze['lock_sha256']
r['owned_equal']=True
active=[]
for line in subprocess.check_output(['ps','-Ao','pid,ppid,command'],text=True).splitlines()[1:]:
 parts=line.strip().split(None,2)
 if len(parts)==3 and int(parts[0]) not in [os.getpid(),os.getppid()] and str(root/'rust/target') in parts[2]:active.append(parts)
assert not active,active;r['own_target_processes_remaining']=[]
(ev/'final-verification.json').write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
allowed=[];excluded=[];flags=[]
suffixes={'.json','.md','.py','.log','.txt','.tsv','.sh','.err','.out','.jsonl','.ndjson','.addr','.pid','.sha256','.pass-lines','.probe-done'}
for f in sorted(ev.rglob('*')):
 if not f.is_file():continue
 rel=str(f.relative_to(ev));reason=None
 if 'synthetic-home' in f.parts:reason='LOCAL_ONLY_SENSITIVE_SYNTHETIC_HOME_NEVER_GIT'
 elif '__pycache__' in f.parts or f.suffix=='.pyc':reason='LOCAL_ONLY_REBUILDABLE_PYTHON_CACHE'
 elif 'dump' in f.name or f.name=='.last-body':reason='LOCAL_ONLY_PROTECTED_RAW_STATE_OR_REPLY'
 elif f.suffix not in suffixes:reason='LOCAL_ONLY_UNCLASSIFIED_NOT_ALLOWLISTED'
 else:
  try:
   body=f.read_text()
   if re.search(r'-----BEGIN (?:EC |RSA |OPENSSH )?PRIVATE KEY-----\s*\n',body) or re.search(r'Bearer\s+(?!\[token\]|<redacted>|redacted|dummy)[A-Za-z0-9._-]{20,}',body):reason='LOCAL_ONLY_POTENTIAL_SECRET_PROTECTED';flags.append(rel)
  except UnicodeDecodeError:reason='LOCAL_ONLY_BINARY_NEVER_GIT'
 row={'path':rel,'sha256':sha(f),'bytes':f.stat().st_size,'classification':reason or 'SAFE_EVIDENCE_EXACT_ALLOWLIST'}
 if reason:excluded.append(row)
 else:allowed.append(row)
assert not flags,flags
m={'at':now(),'status':'READY_FOR_INDEPENDENT_REVIEW','issue_status':'NOT_COMPLETED','safe_to_collect':allowed,'local_only_never_git':excluded,'local_only_root_target':str(root/'rust/target'),'no_full_source_archive_copy':True,'protected_retention':'Synthetic homes kept locally, root mode0700, for original SQLite/wire linkage. Only hashes/path/size exported; never credential bytes. Raw state dumps excluded. No real home or unrelated process touched.','no_secret_header_capture':True,'memory_measured_mb':None,'manifest_self_hash':'Self excluded to avoid circular checksum; hash supplied in final handoff.','final_product_owned':'candidate-manifest.json exact five sources only; parent input/control files not new owned.','preparation_failure_snapshots_are_not_final_hash_indexes':['evidence-manifest-preparation-01.json','final-verification-preparation-01.json']}
f=ev/'evidence-manifest.json';f.write_text(json.dumps(m,ensure_ascii=False,indent=2)+'\n')
for group in ['safe_to_collect','local_only_never_git']:
 for x in m[group]:assert sha(ev/x['path'])==x['sha256']
print(json.dumps({'owned_unchanged':True,'no_owned_process_left':True,'safe_files':len(allowed),'protected_files':len(excluded),'manifest_sha256':sha(f),'candidate_manifest_sha256':sha(ev/'candidate-manifest.json'),'report_sha256':sha(ev/'REPORT.md')},ensure_ascii=False))
