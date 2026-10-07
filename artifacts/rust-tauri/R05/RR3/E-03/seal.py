import json, os, pathlib, re, subprocess
from audit import ROOT, EV, DOCS, load, sha, save, entry, now

assert load(EV/'SELF_CHECK.json')['status']=='PASS'
assert load(EV/'evidence-self-check.json')['status']=='PASS'
last=load(EV/'documents-after.json')
assert [entry(ROOT/p) for p in DOCS]==last
report=EV/'REPORT.md'
for target in re.findall(r'\]\(([^)]+)\)',report.read_text()):
    assert (EV/target.split('#')[0]).exists(),target
# 仅清掉本E导入检查器产生的派生字节；原源码和原证不动。
cache=EV/'__pycache__'
if cache.exists():
    files=list(cache.iterdir());assert all(p.name.startswith('audit.cpython-') and p.suffix=='.pyc' for p in files)
    for p in files:p.unlink()
    cache.rmdir()
head=subprocess.run(['git','rev-parse','HEAD'],cwd=ROOT,capture_output=True,check=True)
branch=subprocess.run(['git','rev-parse','--abbrev-ref','HEAD'],cwd=ROOT,capture_output=True,check=True)
index=ROOT/'.git/index'
g_post=load(ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-02/metadata/post-default.json')
fs=os.statvfs(ROOT)
save('STOPPED.json',dict(at=now(),status='SELF_CHECKED_STOP_WRITING_AFTER_MANIFEST',owner='/root/rr3_e_impl_03',read_only_helper='/root/rr3_e_impl_03/doc_input_audit (completed)',documents_changed=12,documents_owned=14,head=head.stdout.decode().strip(),branch=branch.stdout.decode().strip(),index_sha256=sha(index.read_bytes()),index_comparison='G02 review records e65fd8f9353224483a9b94cc32bca37dde0505f363692af58539d270df9dfc9d; this is readback, no index write',source_and_authority_writes=False,git_writes=False,system_changes=False,builds_started=False,free_bytes=fs.f_bavail*fs.f_frsize,followup='Separate new E independent review; stage NOT_ACCEPTED/R06_READY=false; no further writes by this author.',finalizer_pid=os.getpid(),finalizer='This process only seals the manifest and exits. Actual final tool exit is the completion acknowledgement; no background writer was started.'))
paths=sorted(p for p in EV.rglob('*') if p.is_file() and p.name!='MANIFEST.json')
manifest=dict(at=now(),status='SELF_CHECKED',scope='E03 author evidence only; no independent PASS',source_input_manifest='semantic-inputs-after.json',source_input_digest=load(EV/'semantic-inputs-after.json')['digest'],files=[dict(path=p.relative_to(EV).as_posix(),bytes=p.stat().st_size,sha256=sha(p.read_bytes())) for p in paths],documents_after=last,exclusions=['MANIFEST.json itself (no self-hash cycle)'])
save('MANIFEST.json',manifest)
assert all(sha((EV/r['path']).read_bytes())==r['sha256'] for r in load(EV/'MANIFEST.json')['files'])
print('SELF_CHECKED; manifest',len(paths),'files; all writers stopped after this process exits; R06_READY=false')
