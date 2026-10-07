from pathlib import Path
import datetime,hashlib,json,os,subprocess,shutil
r=Path.cwd();e=r/'artifacts/rust-tauri/R05/RR3/I-01';fixture=e/'selfcheck-04';c=fixture/'runner-copy';p=fixture/'runner-pristine';b=e/'b-production-n03';b.mkdir()
shutil.copytree(r/'docs/rust-tauri',c/'docs/rust-tauri',dirs_exist_ok=True)
shutil.copyfile(r/'scripts/rust-tauri/r05_t08_mutate_pin.py',c/'scripts/rust-tauri/r05_t08_mutate_pin.py')
s=(r/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text();f=(fixture/'production-functions.sh').read_text().split('\neval "$3"\n')[0]
block=s[s.index('record_case()'):s.index('\n# ── controls:')]
cmd=f+'\nEV="$3"; CARGO="$4"; CASE_SCOPE=N03\nnote() { printf "%s\\n" "$*" | tee -a "$EV/summary.txt"; }\n'+block+'''
: > "$EV/case-results.tsv"
mkdir -p "$EV/control-xtask"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask r05_ > "$EV/control-xtask/test.stdout.log" 2>&1
code=$?
echo "$code" > "$EV/control-xtask/exit-code.txt"
[ "$code" -eq 0 ] || fail "normal mirror failed"
run_n03
write_results
'''
q=e/'b-production-command.sh';q.write_text(cmd)
env=dict(os.environ,CARGO_TARGET_DIR=str(fixture/'own-target'),CARGO_NET_OFFLINE='true')
argv=['bash',str(q),str(c),str(p),str(b),str(Path.home()/'.cargo/bin/cargo')];start=datetime.datetime.now(datetime.timezone.utc).isoformat()
with (b/'console.log').open('w') as out:z=subprocess.run(argv,cwd=c,env=env,stdout=out,stderr=subprocess.STDOUT)
receipt={'argv':argv,'cwd':str(c),'startUTC':start,'endUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exitCode':z.returncode,'environmentOverrides':{k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_NET_OFFLINE']},'boundary':'生产原run_n03和原汇总，原shell快照/恢复；隔离RX map不代表全R02/R05业务gate。','gateSha256':hashlib.sha256(s.encode()).hexdigest(),'logSha256':hashlib.sha256((b/'console.log').read_bytes()).hexdigest()}
(b/'command.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n');print(json.dumps(receipt,ensure_ascii=False));assert z.returncode==0
