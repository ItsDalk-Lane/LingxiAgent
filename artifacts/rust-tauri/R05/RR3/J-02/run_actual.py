import datetime, hashlib, json, os, pathlib, shutil, subprocess, tempfile
ROOT=pathlib.Path.cwd().resolve(); EV=ROOT/'artifacts/rust-tauri/R05/RR3/J-02'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def inventory(root):
 rows={}
 for parent,dirs,files in os.walk(root,followlinks=False):
  for name in dirs+files:
   p=pathlib.Path(parent)/name
   if p.is_symlink():rows[str(p.relative_to(root))]={'link':os.readlink(p)}
   elif p.is_file():rows[str(p.relative_to(root))]={'sha256':sha(p),'mode':p.stat().st_mode&0o777}
 return rows
files=['scripts/rust-tauri/prepare_git_copy.py','scripts/rust-tauri/prepare_git_copy_selfcheck.py','scripts/rust-tauri/r05_t08_negative_gate.sh','scripts/rust-tauri/r02_t08_legacy_entry_regression.sh','scripts/rust-tauri/r05_t08_prepare_node.py','scripts/rust-tauri/run_output_sinks.py','scripts/rust-tauri/r02_run_output_regression.py','package.json','package-lock.json','.npmrc']
inputs={p:sha(ROOT/p) for p in files}
(EV/'actual-inputs-before.json').write_text(json.dumps(inputs,indent=2)+'\n')
(EV/'main-git-before.json').write_text(json.dumps(inventory(ROOT/'.git'),indent=2)+'\n')
(ev:=EV/'full-default-01').mkdir()
work=pathlib.Path.home()/'r05t08-work';work.mkdir(exist_ok=True)
copy=pathlib.Path(tempfile.mkdtemp(prefix='j02-default-',dir=work))
(EV/'full-copy-path.txt').write_text(str(copy)+'\n')
shell=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text()
start=shell.index('SOURCE_HEAD="');end=shell.index('\n# Pristine copies',start)
fragment='set -uo pipefail; ROOT="$1"; COPY="$2"; EV="$3"; note(){ echo "$*"; }; fail(){ echo "FAIL: $*" >&2; exit 1; };\n'+shell[start:end]
(EV/'full-default-prefix.sh').write_text(fragment)
argv=['bash',str(EV/'full-default-prefix.sh'),str(ROOT),str(copy),str(ev)]
record=dict(argv=argv,cwd=str(ROOT),startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),freeBefore=shutil.disk_usage(ROOT).free)
with (EV/'full-default.stdout').open('wb') as out,(EV/'full-default.stderr').open('wb') as err:
 p=subprocess.run(argv,cwd=ROOT,stdout=out,stderr=err)
record.update(exitCode=p.returncode,finishedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),freeAfter=shutil.disk_usage(ROOT).free,stdoutSha256=sha(EV/'full-default.stdout'),stderrSha256=sha(EV/'full-default.stderr'))
(EV/'full-default-command.json').write_text(json.dumps(record,indent=2)+'\n')
after=inventory(ROOT/'.git');(EV/'main-git-after.json').write_text(json.dumps(after,indent=2)+'\n')
before=json.loads((EV/'main-git-before.json').read_text())
(EV/'main-preserved.json').write_text(json.dumps(dict(gitDifferences={k:dict(before=before.get(k),after=after.get(k)) for k in before.keys()|after.keys() if before.get(k)!=after.get(k)},inputDifferences=[k for k,v in inputs.items() if sha(ROOT/k)!=v]),indent=2)+'\n')
print(json.dumps(record));print((EV/'main-preserved.json').read_text());raise SystemExit(p.returncode)
