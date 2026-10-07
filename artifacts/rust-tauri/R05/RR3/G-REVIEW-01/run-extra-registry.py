import pathlib,json,sys,hashlib
# 复用本轮命令记录器，只在同一自有副本注入；依赖恢复动作顺序执行。
ns=globals()
def extra_registry(run,C,E,cargo):
 pin=C/'docs/rust-tauri/R05/r05_stage_cids.tsv';req=C/'docs/rust-tauri/R05/r05_required_cids.tsv';leaf=C/'docs/rust-tauri/R05/r05_leaf_case_map.tsv';mp=C/'rust/crates/xtask/src/stage_maps/R05.json'
 originals={p:p.read_bytes() for p in [pin,req,leaf,mp]}
 cases=[]
 try:
  for name in ['F26-cid-rename','F26-registry-missing','F26-bogus-command','F25-exclusive-share','F25-leafcase-missing']:
   for p,b in originals.items():p.write_bytes(b)
   if name=='F26-cid-rename':
    text=pin.read_text();assert 'R05-T02-C01' in text;pin.write_text(text.replace('R05-T02-C01','R05-T99-C99',1));f='r05_stage_cid_table_owns'
   elif name=='F26-registry-missing':
    lines=req.read_text().splitlines(True);hits=[l for l in lines if l.startswith('reqcid R05-T04-C07 ')];assert len(hits)==1;req.write_text(''.join(l for l in lines if l not in hits));f='r05_stage_cid_table_owns'
   elif name=='F26-bogus-command':
    text=req.read_text();old='reqcid R05-T08-C13 command:r04_regression_gate';assert text.count(old)==1;req.write_text(text.replace(old,'reqcid R05-T08-C13 command:bogus_gate',1));f='r05_stage_cid_table_owns'
   elif name=='F25-exclusive-share':
    d=json.loads(mp.read_text());matches=[x for x in d['supplementalLeafScenarios'] if x['id']=='R00-T02-LA-16CEB6D12A6A'];assert len(matches)==1;matches[0]['basisKind']='stage_share_satisfied';mp.write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n');f='r05_production_map_mirrors'
   else:
    lines=leaf.read_text().splitlines(True);hits=[l for l in lines if 'r05-full-16CEB6D12A6A-a1' in l];assert len(hits)==1;leaf.write_text(''.join(l for l in lines if l not in hits));f='r05_production_map_mirrors'
   mutations=[{'path':str(p.relative_to(C)),'beforeSHA256':hashlib.sha256(b).hexdigest(),'afterSHA256':hashlib.sha256(p.read_bytes()).hexdigest()} for p,b in originals.items() if p.read_bytes()!=b]
   ec=run(name,[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--bin','xtask',f]);cases.append({'case':name,'exitCode':ec,'mutations':mutations})
   for p,b in originals.items():p.write_bytes(b)
   run(name+'-restored',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--bin','xtask',f])
  for p,b in originals.items():p.write_bytes(b)
  lines=pin.read_text().splitlines(True);hits=[l for l in lines if l.startswith('cid R05-T04-C07 ')];assert len(hits)==1;pin.write_text(''.join(l for l in lines if l not in hits))
  ec=run('F26-producer-preflight',['bash','scripts/rust-tauri/r05_t08_stage_suites.sh',str(E/'supplements/F26-producer-preflight/suites')]);cases.append({'case':'F26-producer-preflight','exitCode':ec})
 finally:
  for p,b in originals.items():p.write_bytes(b)
 (E/'extra-registry-mutations.json').write_text(json.dumps({'cases':cases,'allRestored':all(p.read_bytes()==b for p,b in originals.items()),'boundary':'真实当前生产镜像和生产者预检，不改主树或生产需求；完整producer正常控与当前目标恢复测试共同提供恢复绿'},ensure_ascii=False,indent=2)+'\n')
