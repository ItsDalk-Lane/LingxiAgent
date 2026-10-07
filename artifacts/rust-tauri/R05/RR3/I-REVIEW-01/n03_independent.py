from review_support import *
import shutil,re
f=EV/'permanent-final';c=f/'runner-copy';p=f/'runner-pristine';b=EV/'n03-independent';b.mkdir()
shutil.copytree(ROOT/'docs/rust-tauri',c/'docs/rust-tauri',dirs_exist_ok=True)
for name in ['r05_t08_mutate_pin.py','r05_t08_negative_gate.sh']:
 shutil.copyfile(ROOT/'scripts/rust-tauri'/name,c/'scripts/rust-tauri'/name)
s=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text()
base=(f/'production-functions.sh').read_text().split('\neval "$3"\n')[0]
block=s[s.index('record_case()'):s.index('\n# ── controls:')]
q=EV/'n03-independent.sh';q.write_text(base+'\nEV="$3"; CARGO="$4"; CASE_SCOPE=N03\nnote() { printf "%s\\n" "$*" | tee -a "$EV/summary.txt"; }\n'+block+'''
: > "$EV/case-results.tsv"
mkdir -p "$EV/control-xtask"
cargo_in_copy test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask r05_ > "$EV/control-xtask/test.stdout.log" 2>&1
code=$?
echo "$code" > "$EV/control-xtask/exit-code.txt"
[ "$code" -eq 0 ] || fail "normal mirror failed"
run_n03
write_results
''')
env=dict(os.environ,CARGO_TARGET_DIR=str(f/'own-target'),CARGO_NET_OFFLINE='true')
inputs={str(x.relative_to(c)):sha(x) for sub in ['rust','scripts','docs'] for x in (c/sub).rglob('*') if x.is_file() and '__pycache__' not in str(x)}
save('n03-inputs-before.json',inputs)
run('n03-production-functions',['bash',q,c,p,b,Path.home()/'.cargo/bin/cargo'],cwd=c,env=env,expected=0)
counts={}
for name,expected in [('control-xtask',(8,0,0,113)),('n03-tamper-pin-count',(0,1,0,120)),('n03-restored',(1,0,0,120))]:
 log=b/name/'test.stdout.log';text=log.read_text()
 found=re.findall(r'test result: .*? (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; (\d+) filtered',text)
 assert len(found)==1 and tuple(map(int,found[0]))==expected,(name,found)
 binary=re.search(r'Running .*?\(([^\n]+)\)',text).group(1)
 binary=Path(binary);binary=binary if binary.is_absolute() else c/binary
 counts[name]={'actualCounts':list(map(int,found[0])),'logSha256':sha(log),'binary':str(binary),'binarySha256':sha(binary),'exitCode':int((b/name/'exit-code.txt').read_text())}
m=json.loads((b/'n03-tamper-pin-count/mutation.json').read_text());assert m['old']==24 and m['new']==23 and m['matches']==m['mutations']==1
summary=json.loads((b/'case-results.json').read_text());assert len(summary['cases'])==1 and len(summary['unexecutedCases'])==15 and summary['allRefused'] and summary['controlsGreen']
after={key:sha(c/key) for key in inputs};assert after==inputs
assert len({v['binarySha256'] for v in counts.values()})==1
save('n03-independent-result.json',{'counts':counts,'mutation':m,'summary':summary,'inputsRestored':True,'boundary':'真实cargo/原run_n03/reset/write_results函数；不执行clone准备/共享NEG_TARGET/其他15项。完整入口须新G。'})
