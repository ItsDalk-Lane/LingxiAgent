import pathlib,subprocess,json,hashlib,os,tempfile,shutil,difflib
W=pathlib.Path.cwd();O=pathlib.Path(__file__).resolve().parent;D=W/'rust/target/debug/deps';F=W/'rust/target/debug/.fingerprint';env=os.environ.copy();env['CARGO_PKG_VERSION']='0.0.0';env['CARGO_MANIFEST_DIR']=str(W/'rust/crates/lingxi-service')
h=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
# 直接使用固定候选编译依赖，不往候选 target 写入任何内容。
def deps(fp):
 out=[]
 for _,name,_,wanted in json.loads(fp.read_text())['deps']:
  choices=[]
  for q in F.glob('*/lib-'+name):
   if int.from_bytes(bytes.fromhex(q.read_text()),'little')==wanted:
    lib=D/('lib'+name+'-'+q.parent.name.rsplit('-',1)[1]+'.rlib')
    if lib.exists():choices.append(lib)
  assert len(choices)==1,(name,choices)
  out.extend(['--extern',name+'='+str(choices[0])])
 return out
fp=F/'lingxi-service-7547adafbd47d443/lib-lingxi_service.json';args=deps(fp)
source=subprocess.check_output(['git','show','2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d:rust/crates/lingxi-service/src/context_compiler.rs'],text=True)
a=source.index('pub fn scan_agent_config(');b=source.index('/// preferences.json',a);old=source[a:b].rstrip();old=old[:old.index('///')].rstrip() if '///' in old else old
# 旧函数是无Result的逐行读取器；保留新公开接口，生产文件读取仅在临时源副本改回旧读取器。
old=old.replace('pub fn scan_agent_config(', 'fn legacy_scan_agent_config(')
original=(W/'rust/crates/lingxi-service/src/context_compiler.rs').read_text();a=original.index('pub fn scan_agent_config(');b=original.index('/// preferences.json',a)
replacement=old+'\n\npub fn scan_agent_config(text:&str)->Result<AgentConfigScan,lingxi_adapters::storage::agent_memory::MemoryError>{Ok(legacy_scan_agent_config(text))}\nfn parse_agent_material(path:&Path)->Result<AgentConfigScan,ContextMaterialError>{let text=lingxi_adapters::storage::agent_memory::config::read_config_text(path).map_err(|err|ContextMaterialError::Parse{path:path.to_path_buf(),detail:err.to_string()})?;Ok(legacy_scan_agent_config(&text))}\n\n'
mutated=original
(O/'gateway-extra-source.diff').write_text(''.join(difflib.unified_diff(original.splitlines(True),mutated.splitlines(True),fromfile='fixed/context_compiler.rs',tofile='temporary-mutant/context_compiler.rs')))
with tempfile.TemporaryDirectory(prefix='lingxi-r03-old-flow-') as temp:
 root=pathlib.Path(temp);src=root/'src';shutil.copytree(W/'rust/crates/lingxi-service/src',src);(src/'context_compiler.rs').write_text(mutated)
 rlib=root/'liblingxi_service_mutant.rlib';rustc='/Users/study_superior/.cargo/bin/rustc';cmd=[rustc,'--crate-name','lingxi_service','--edition=2021',str(src/'lib.rs'),'--crate-type','rlib','-C','debuginfo=0','-L','dependency='+str(D),*args,'-o',str(rlib)]
 log=O/'logs/gateway-extra-build.log'
 with log.open('w') as f:r=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT)
 record={'compile_command':cmd,'compile_exit':r.returncode,'source_sha':hashlib.sha256(mutated.encode()).hexdigest(),'original_source_sha':hashlib.sha256(original.encode()).hexdigest(),'temporary_source_only':True,'borrowed_dependency_rlibs_readonly':True}
 if r.returncode==0:
  t=W/'rust/crates/lingxi-service/tests/r06_ar_f06_config.rs';ts=t.read_text().replace('        resource_depth.as_str(),','        resource_depth.as_str(),\n        \"future: !!null wrong\",\n        \"future: !!int 0B10\",').replace('include_str!("../../../../lib/config.example.yaml")','include_str!("'+str(W/'lib/config.example.yaml')+'")');testsrc=root/'test.rs';testsrc.write_text(ts)
  testfp=next(f for f in F.glob('lingxi-service-*/test-integration-test-r06_ar_f06_config.json') if (D/('r06_ar_f06_config-'+f.parent.name.rsplit('-',1)[1])).is_file());testargs=deps(testfp);testargs=[v.replace(str(D/'liblingxi_service-7547adafbd47d443.rlib'),str(rlib)) for v in testargs]
  exe=root/'old-flow-test';cmd2=[rustc,'--test','--crate-name','r03_mutant','--edition=2021',str(testsrc),'-C','debuginfo=0','-L','dependency='+str(D),*testargs,'-o',str(exe)]
  with (O/'logs/gateway-extra-test-build.log').open('w') as f:r2=subprocess.run(cmd2,env=env,stdout=f,stderr=subprocess.STDOUT)
  record.update(test_compile_command=cmd2,test_compile_exit=r2.returncode)
  if r2.returncode==0:
   env.update(TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',LINGXI_TEST_GUARD='1',TEST_GUARD_VERBOSE='1');home=root/'home';home.mkdir();env['LINGXI_HOME']=str(home)
   with (O/'logs/gateway-extra-behavior.log').open('w') as f:r3=subprocess.run([str(exe),'p02_real_install_gateway_checks_config_before_install_and_enable','--nocapture'],env=env,stdout=f,stderr=subprocess.STDOUT)
   record.update(behavior_exit=r3.returncode,behavior_command=[str(exe),'p02_real_install_gateway_checks_config_before_install_and_enable','--nocapture'],binary_sha=h(exe),guard='8192MB/600s')
 (O/'gateway-extra-results.json').write_text(json.dumps(record,indent=2));print({k:v for k,v in record.items() if k.endswith('exit')})
