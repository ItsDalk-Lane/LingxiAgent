from review_capture import *
base=ROOT/'rust/target/debug/.fingerprint'; deps=ROOT/'rust/target/debug/deps'; iso=EV/'isolated'; source=iso/'rust/crates/lingxi-service'; out=iso/'build';out.mkdir(exist_ok=True)
meta=json.loads((base/'lingxi-service-d1b960a1b85442c2/lib-lingxi_service.json').read_text())
extern=[];inputs={}
for _,name,_,value in meta['deps']:
 hits=[]
 for f in base.glob('*/lib-'+name):
  try:
   if int.from_bytes(bytes.fromhex(f.read_text()),'little')==value:hits.append(f)
  except ValueError:pass
 assert len(hits)==1,(name,hits)
 suffix=hits[0].parent.name.rsplit('-',1)[-1];p=deps/f'lib{name}-{suffix}.rlib';assert p.is_file(),p
 extern+=['--extern',f'{name}={p}'];inputs[str(p)]=sha(p)
native=[]
for p in (ROOT/'rust/target/debug/build').glob('*/out'):
 if p.is_dir():native+=['-L',f'native={p}']
label=sys.argv[1]; ev=EV/label;ev.mkdir();env=os.environ.copy();env.update({'CARGO_PKG_VERSION':'0.0.0','CARGO_MANIFEST_DIR':str(source),'CARGO_PKG_NAME':'lingxi-service'})
rustc='/Users/study_superior/.cargo/bin/rustc';common=['--edition=2021','-C','debuginfo=0','-L',f'dependency={deps}']
commands=[[rustc,'--crate-name','lingxi_service',str(source/'src/lib.rs'),'--crate-type','lib','-C','metadata=rr3cf46review',*common,*extern,*native,'-o',str(out/'liblingxi_service.rlib')], [rustc,'--crate-name','lingxi_service_binary',str(source/'src/main.rs'),*common,*extern,'--extern',f'lingxi_service={out}/liblingxi_service.rlib',*native,'-o',str(out/'lingxi-service')]]
records=[]
for index,cmd in enumerate(commands):
 start=utc()
 with (ev/f'build-{index}.log').open('wb') as f:p=subprocess.run(cmd,cwd=iso,env=env,stdout=f,stderr=subprocess.STDOUT)
 records.append({'command':cmd,'startedAt':start,'endedAt':utc(),'exitCode':p.returncode,'stdoutHash':sha(ev/f'build-{index}.log')})
 if p.returncode:break
record={'records':records,'isolatedLoggingHash':sha(source/'src/logging.rs'),'productionLoggingHash':sha(ROOT/'rust/crates/lingxi-service/src/logging.rs'),'lockSha256':sha(iso/'rust/Cargo.lock'),'dependencyArtifacts':inputs,'method':'strict copied complete service lib + main; rustc 1.98.1, exact existing --locked Cargo dependency artifacts reused by fingerprint identity; no target tree copied; output only review evidence','binarySha256':sha(out/'lingxi-service') if (out/'lingxi-service').is_file() else None}
(ev/'command.json').write_text(json.dumps(record,indent=2));print({'exitCodes':[r['exitCode'] for r in records],'binary':record['binarySha256']});sys.exit(records[-1]['exitCode'])
