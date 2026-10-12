import pathlib,subprocess,json,hashlib,tempfile,os,sys
c=pathlib.Path.cwd();o=pathlib.Path(__file__).parent;dep=c/"rust/target/debug/deps";td=pathlib.Path(tempfile.mkdtemp(prefix="w12-r02-binaries-"));records=[]
base="2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d";old=subprocess.check_output(["git","show",base+":rust/crates/lingxi-adapters/src/storage/knowledge.rs"],text=True)
for kind,source,names in [("status",(o/"counter-status.rs").read_text(),["lingxi_adapters","rusqlite","lingxi_test_guard"]),("old-open","mod old {\n"+old+"\n}\n"+(o/"old-rust-main.rs").read_text(),["lingxi_kernel","rusqlite","serde","serde_json","tracing","getrandom","lingxi_test_guard"]),("new-open",(o/"old-rust-main.rs").read_text().replace("old::", "lingxi_adapters::storage::knowledge::"),["lingxi_adapters","rusqlite","lingxi_test_guard"])]:
 binary=td/kind;args=["/Users/study_superior/.cargo/bin/rustc","--edition=2021","--crate-name","review_"+kind.replace("-","_"),"-","-L","dependency="+str(dep),"-A","dead_code","-A","unused_imports","-o",str(binary)];extern=[]
 for name in names:
  choices=list(dep.glob("lib"+name+"-*.rlib"));p=(dep/"libgetrandom-8a034aa6a469b884.rlib") if name=="getrandom" else max(choices,key=lambda x:x.stat().st_mtime);args +=["--extern",name+"="+str(p)];extern.append({"path":str(p),"sha256":hashlib.sha256(p.read_bytes()).hexdigest()})
 r=subprocess.run(args,input=source,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=600);print(r.stdout);assert r.returncode==0
 records.append({"kind":kind,"path":str(binary),"sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"source_sha256":hashlib.sha256(source.encode()).hexdigest(),"old_source_sha256":hashlib.sha256(old.encode()).hexdigest() if kind=="old-open" else None,"argv":args,"extern":extern})
(o/"probe-binaries-before.json").write_text(json.dumps(records,indent=2));print(json.dumps(records));
