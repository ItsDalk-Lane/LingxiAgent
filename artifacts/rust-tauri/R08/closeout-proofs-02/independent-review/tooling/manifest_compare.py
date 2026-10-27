import json,hashlib,os,sys
G='artifacts/rust-tauri/R08/verify-r08-closeout-08/'
d=json.load(open(G+'candidate-source-before.json'))
changed=[];missing=[];extra=[]
seen=set()
for e in d['entries']:
    p=bytes.fromhex(e['pathBytesHex']).decode('utf-8','surrogateescape')
    seen.add(p)
    full=os.path.join('.',p)
    try:
        with open(full,'rb') as f:
            h=hashlib.sha256(f.read()).hexdigest()
    except FileNotFoundError:
        missing.append(p); continue
    except IsADirectoryError:
        missing.append(p+' (dir)'); continue
    except Exception as ex:
        missing.append(f'{p} ({ex})'); continue
    if h!=e['sha256']:
        changed.append(p)
# find current files not in manifest under tracked dirs (skip .git, node_modules, artifacts evidence, rust/target)
skip=('.git/','node_modules/','rust/target/','dist/','artifacts/rust-tauri/R08/verify-r08-closeout-0','artifacts/rust-tauri/R08/closeout-proofs-0')
for root,dirs,files in os.walk('.'):
    dirs[:]=[x for x in dirs if not (os.path.join(root,x)+'/').lstrip('./').startswith(('.git/','node_modules/','rust/target/')) and x not in ('.git','node_modules','target')]
    for fn in files:
        p=os.path.relpath(os.path.join(root,fn),'.')
        if any(p.startswith(s) for s in skip): continue
        if p not in seen: extra.append(p)
print(json.dumps({'changed':changed,'missing':missing,'extra_count':len(extra),'extra_sample':extra[:40]},indent=1))
