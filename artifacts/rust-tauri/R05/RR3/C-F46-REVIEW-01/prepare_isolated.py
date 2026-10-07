from review_capture import *
iso=EV/'isolated';iso.mkdir(); src=iso/'rust';(src/'crates').mkdir(parents=True)
for name in ['lingxi-service','lingxi-kernel','lingxi-adapters','lingxi-protocol','lingxi-spike','lingxi-browser-spike','xtask']:
 shutil.copytree(ROOT/'rust/crates'/name,src/'crates'/name,ignore=shutil.ignore_patterns('target'))
for p in (ROOT/'rust').iterdir():
 if p.is_file():shutil.copy2(p,src/p.name)
shutil.copy2(ROOT/'rust-toolchain.toml',iso/'rust-toolchain.toml')
# 只复用既有依赖的只读定位，实际编译输出必须落在本包。
# 本轮采用独立 rustc 编译，只复制生产源，不复制 target 树。
(EV/'isolated-source-manifest.json').write_text(json.dumps({str(p.relative_to(iso)):sha(p) for p in src.rglob('*') if p.is_file()},indent=2))
print('isolated source prepared')
