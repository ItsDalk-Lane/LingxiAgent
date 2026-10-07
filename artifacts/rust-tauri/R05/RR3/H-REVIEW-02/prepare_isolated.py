from capture import *
import shutil

iso=EV/'isolated'
iso.mkdir(exist_ok=False)
(iso/'rust').mkdir()
shutil.copytree(ROOT/'rust/crates',iso/'rust/crates',ignore=shutil.ignore_patterns('target','__pycache__'))
for p in (ROOT/'rust').iterdir():
    if p.is_file(): shutil.copy2(p,iso/'rust'/p.name)
shutil.copy2(ROOT/'rust-toolchain.toml',iso/'rust-toolchain.toml')
(iso/'scripts/rust-tauri').mkdir(parents=True)
for name in ['r02_t01_service_smoke.sh','r02_t07_redaction_scan.sh']:
    shutil.copy2(ROOT/'scripts/rust-tauri'/name,iso/'scripts/rust-tauri'/name)
write(EV/'isolated-source-before.json',{str(p.relative_to(iso)):sha(p) for p in iso.rglob('*') if p.is_file()})
print({'isolated':str(iso),'sourceFiles':sum(p.is_file() for p in iso.rglob('*')),'targetCopied':False,'gitCopied':False})
